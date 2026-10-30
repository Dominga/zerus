//! One selected task input, sharing the application's ordered draft store and
//! private attachment cache. Each explicit operation is executed once.
use crate::composer::{
    fetch::{Fetched, Fetcher},
    Composers,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
};
use tauri::State;
use zerus_client_core::{
    attachments::{self, Draft, Selection},
    drafts::{DraftStore, Loaded, StoredDraft},
    reads::CachedAttachment,
    task_details,
    tasks_write::{self, outbox, TaskCall},
};

const CHANGED: &str = "The selected task changed. Your draft was kept.";
const TOO_LONG: &str = "Message exceeds 64 KiB.";
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Note,
    Description,
}
impl Mode {
    fn prefix(self) -> &'static str {
        if self == Self::Description {
            "task-description"
        } else {
            "task-comment"
        }
    }
}
#[derive(Clone, Serialize)]
pub struct EditorFile {
    #[serde(flatten)]
    file: Fetched,
    reference: String,
}
#[derive(Clone, Serialize)]
pub struct View {
    handle: u64,
    rev: u64,
    project: String,
    task: String,
    mode: Mode,
    text: String,
    position: i64,
    anchor: i64,
    files: Vec<EditorFile>,
    error: String,
    recovery: bool,
    blocked: bool,
    saved: bool,
    unsaved: bool,
}
struct Pins {
    cache: Arc<crate::composer::files::Cache>,
    paths: Vec<PathBuf>,
}
impl Drop for Pins {
    fn drop(&mut self) {
        for path in &self.paths {
            self.cache.unpin(path);
        }
    }
}
struct Slot {
    handle: u64,
    rev: u64,
    project: String,
    task: String,
    mode: Mode,
    stored: StoredDraft,
    files: Vec<EditorFile>,
    pins: Option<Pins>,
    unsaved: bool,
    error: String,
    recovery: bool,
    blocked: bool,
}
impl Slot {
    fn key(&self) -> String {
        format!("{}\n{}\n{}", self.mode.prefix(), self.project, self.task)
    }
}
#[derive(Default)]
struct Inner {
    slot: Mutex<Option<Slot>>,
    busy: AtomicBool,
    handles: AtomicU64,
}
#[derive(Clone, Default)]
pub struct TaskEditor(Arc<Inner>);
struct Permit(TaskEditor);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0 .0.busy.store(false, Ordering::SeqCst);
    }
}
impl TaskEditor {
    fn begin(&self) -> Result<Permit, String> {
        self.0
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "This task already has an operation in progress.".to_owned())?;
        Ok(Permit(self.clone()))
    }
}
fn load(store: &DraftStore, key: &str) -> Result<Loaded, String> {
    let (tx, rx) = mpsc::channel();
    store.load(key, move |value| {
        let _ = tx.send(value);
    });
    rx.recv()
        .map_err(|_| "The draft could not be loaded.".into())
}
fn save(store: &DraftStore, key: &str, value: StoredDraft) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    store.save_durable(key, value, move |result| {
        let _ = tx.send(result);
    });
    rx.recv()
        .map_err(|_| "The draft could not be saved.".to_owned())?
}
fn checkpoint(slot: &mut Slot, store: &DraftStore) -> Result<(), String> {
    slot.unsaved = true;
    if let Err(error) = save(store, &slot.key(), slot.stored.clone()) {
        slot.error = error.clone();
        return Err(error);
    }
    slot.unsaved = false;
    Ok(())
}
fn clear(store: &DraftStore, key: &str, expected: Draft) -> Result<bool, String> {
    let (tx, rx) = mpsc::channel();
    store.clear_if_unchanged(key, expected, move |result| {
        let _ = tx.send(result);
    });
    rx.recv()
        .map_err(|_| "The saved draft could not be cleared.".to_owned())?
}
fn document(project: &str, task: &str) -> Result<Value, String> {
    let reply = tasks_write::run(
        &zerus_client_core::default_hgs(),
        &task_details::document_call(project, task)?,
    )?;
    task_details::checked(&reply, project, task)
}
fn refresh_files(slot: &mut Slot, composers: &Composers, fetcher: &Fetcher) -> Result<(), String> {
    let cache = composers.cache();
    let prepared = attachments::prepare(cache.root(), &slot.stored.draft.attachments)?;
    let pins = Pins {
        cache: Arc::clone(&cache),
        paths: prepared.iter().map(|f| f.local_path.clone()).collect(),
    };
    for path in &pins.paths {
        cache.pin(path);
    }
    let files = prepared
        .into_iter()
        .map(|file| {
            let (shown, _) = fetcher.described(CachedAttachment {
                local_path: file.local_path,
                metadata: json!({"name":file.name,"mime":file.mime}),
            })?;
            Ok(EditorFile {
                file: shown,
                reference: file.reference,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    slot.files = files;
    slot.pins = Some(pins);
    Ok(())
}
fn view(slot: &Slot) -> View {
    View {
        handle: slot.handle,
        rev: slot.rev,
        project: slot.project.clone(),
        task: slot.task.clone(),
        mode: slot.mode,
        text: slot.stored.draft.text.clone(),
        position: slot.stored.position,
        anchor: slot.stored.anchor,
        files: slot.files.clone(),
        error: slot.error.clone(),
        recovery: slot.recovery,
        blocked: slot.blocked,
        saved: false,
        unsaved: slot.unsaved,
    }
}
fn writable(detail: &Value, mode: Mode) -> Result<(), String> {
    if detail["writable"] == false {
        return Err("Update Zerus to edit this task.".into());
    }
    if mode == Mode::Description && !matches!(detail["stage"].as_str(), Some("todo" | "backlog")) {
        return Err(
            "Description, attachments and assignment can be edited in To do or Backlog.".into(),
        );
    }
    Ok(())
}
fn open_slot(
    editor: &TaskEditor,
    composers: &Composers,
    project: String,
    task: String,
    mode: Mode,
    read: impl Fn(&str, &str) -> Result<Value, String>,
) -> Result<Slot, String> {
    {
        let mut guard = crate::composer::lock(&editor.0.slot);
        if let Some(old) = guard.as_mut().filter(|s| s.unsaved) {
            checkpoint(old, &composers.0.store)?;
        }
    }
    let key = format!("{}\n{project}\n{task}", mode.prefix());
    let loaded = load(&composers.0.store, &key)?;
    let mut stored = loaded.draft;
    let blocked = loaded.storage_error.is_some();
    if !blocked
        && mode == Mode::Description
        && !stored.sending
        && !stored.uncertain
        && stored.draft.text.is_empty()
        && stored.draft.attachments.is_empty()
    {
        let detail = read(&project, &task)?;
        writable(&detail, mode)?;
        let files = tasks_write::decode_files(&detail["attachments"]);
        if detail["attachments"]
            .as_array()
            .is_some_and(|a| a.len() != files.len())
        {
            return Err("Task attachments could not be read.".into());
        }
        stored.draft.text = detail["text"].as_str().unwrap_or("").into();
        stored.draft.attachments = files;
        if stored.draft.text.len() > crate::composer::MAX_TEXT_BYTES {
            return Err(TOO_LONG.into());
        }
        save(&composers.0.store, &key, stored.clone())?;
    }
    let pending = if mode == Mode::Note {
        outbox::pending(composers.0.store.root(), &project, &task)
    } else {
        Ok(None)
    };
    let recovery = pending.as_ref().is_ok_and(Option::is_some);
    let error = loaded
        .storage_error
        .or_else(|| pending.err())
        .or(loaded.notice)
        .unwrap_or_default();
    let slot = Slot {
        handle: editor.0.handles.fetch_add(1, Ordering::SeqCst) + 1,
        rev: 0,
        project,
        task,
        mode,
        stored,
        files: Vec::new(),
        pins: None,
        unsaved: false,
        error,
        recovery,
        blocked,
    };
    Ok(slot)
}
#[tauri::command]
pub async fn task_editor_open(
    state: State<'_, TaskEditor>,
    composers: State<'_, Composers>,
    fetcher: State<'_, Arc<Fetcher>>,
    project: String,
    task: String,
    mode: Mode,
) -> Result<View, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    task_details::document_call(&project, &task)?;
    let permit = state.begin()?;
    let editor = state.inner().clone();
    let composers = composers.inner().clone();
    let fetcher = Arc::clone(fetcher.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let mut slot = open_slot(&editor, &composers, project, task, mode, document)?;
        refresh_files(&mut slot, &composers, &fetcher)?;
        let answer = view(&slot);
        *crate::composer::lock(&editor.0.slot) = Some(slot);
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
    if input.handle != slot.handle || input.rev < slot.rev {
        return Err(CHANGED.into());
    }
    if slot.blocked {
        return Err(zerus_client_core::drafts::NOT_RESTORED.into());
    }
    if input.text.len() > crate::composer::MAX_TEXT_BYTES {
        return Err(TOO_LONG.into());
    }
    let units = input.text.encode_utf16().count() as i64;
    if slot.mode == Mode::Description && slot.stored.draft.text != input.text {
        slot.stored.uncertain = false;
        slot.stored.sending = false;
        slot.error.clear();
    }
    slot.stored.draft.text = input.text;
    slot.stored.position = input.position.clamp(0, units);
    slot.stored.anchor = input.anchor.clamp(0, units);
    slot.rev = input.rev;
    Ok(())
}
#[tauri::command]
pub async fn task_editor_edit(
    state: State<'_, TaskEditor>,
    composers: State<'_, Composers>,
    fetcher: State<'_, Arc<Fetcher>>,
    input: Edit,
) -> Result<View, String> {
    let permit = state.begin()?;
    let editor = state.inner().clone();
    let composers = composers.inner().clone();
    let _ = fetcher;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let mut guard = crate::composer::lock(&editor.0.slot);
        let slot = guard.as_mut().ok_or(CHANGED)?;
        edited(slot, input)?;
        checkpoint(slot, &composers.0.store)?;
        Ok(view(slot))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileChange {
    Attach {
        paths: Vec<PathBuf>,
        start: usize,
        end: usize,
    },
    Remove {
        index: usize,
    },
}
fn files(slot: &mut Slot, change: FileChange) -> Result<(), String> {
    if slot.blocked {
        return Err(zerus_client_core::drafts::NOT_RESTORED.into());
    }
    let mut draft = slot.stored.draft.clone();
    let (mut position, mut anchor) = (slot.stored.position, slot.stored.anchor);
    match change {
        FileChange::Remove { index } => {
            draft.remove(index).ok_or("The attachment changed.")?;
        }
        FileChange::Attach { paths, start, end } => {
            if paths.len() > attachments::MAX_ATTACHMENTS {
                return Err("Attach up to 8 files.".into());
            }
            let mut selection = Selection { start, end };
            for path in paths {
                let file = attachments::read_file(&path)?;
                let added = draft.add(&file.name, &file.mime, file.bytes, selection)?;
                selection = Selection {
                    start: added.cursor,
                    end: added.cursor,
                };
            }
            position = selection.start as i64;
            anchor = selection.end as i64;
        }
    }
    if draft.text.len() > crate::composer::MAX_TEXT_BYTES {
        return Err(TOO_LONG.into());
    }
    if slot.mode == Mode::Description {
        slot.stored.uncertain = false;
        slot.stored.sending = false;
        slot.error.clear();
    }
    slot.stored.draft = draft;
    slot.stored.position = position;
    slot.stored.anchor = anchor;
    slot.rev += 1;
    Ok(())
}
fn change_files(
    slot: &mut Slot,
    composers: &Composers,
    fetcher: &Fetcher,
    change: FileChange,
) -> Result<(), String> {
    let original = slot.stored.clone();
    let rev = slot.rev;
    files(slot, change)?;
    if let Err(error) = refresh_files(slot, composers, fetcher) {
        slot.stored = original;
        slot.rev = rev;
        return Err(error);
    }
    let _ = checkpoint(slot, &composers.0.store);
    Ok(())
}
#[tauri::command]
pub async fn task_editor_files(
    state: State<'_, TaskEditor>,
    composers: State<'_, Composers>,
    fetcher: State<'_, Arc<Fetcher>>,
    handle: u64,
    change: FileChange,
) -> Result<View, String> {
    let permit = state.begin()?;
    let editor = state.inner().clone();
    let composers = composers.inner().clone();
    let fetcher = Arc::clone(fetcher.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let mut guard = crate::composer::lock(&editor.0.slot);
        let slot = guard
            .as_mut()
            .filter(|s| s.handle == handle)
            .ok_or(CHANGED)?;
        change_files(slot, &composers, &fetcher, change)?;
        Ok(view(slot))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn task_editor_pick(
    app: tauri::AppHandle,
    state: State<'_, TaskEditor>,
    composers: State<'_, Composers>,
    fetcher: State<'_, Arc<Fetcher>>,
    handle: u64,
    start: usize,
    end: usize,
) -> Result<View, String> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;
    // The lane stays reserved while the native picker is open; a stale handle
    // cannot append its files to another task's draft.
    let permit = state.begin()?;
    let editor = state.inner().clone();
    let composers = composers.inner().clone();
    let fetcher = Arc::clone(fetcher.inner());
    let parent = app.get_webview_window(crate::MAIN_WINDOW);
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        {
            let guard = crate::composer::lock(&editor.0.slot);
            if guard
                .as_ref()
                .is_none_or(|s| s.handle != handle || s.blocked)
            {
                return Err(CHANGED.into());
            }
        }
        let mut dialog = app.dialog().file().set_title("Attach files");
        if let Some(parent) = &parent {
            dialog = dialog.set_parent(parent);
        }
        let paths = dialog
            .blocking_pick_files()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.into_path().ok())
            .collect::<Vec<_>>();
        let mut guard = crate::composer::lock(&editor.0.slot);
        let slot = guard
            .as_mut()
            .filter(|s| s.handle == handle)
            .ok_or(CHANGED)?;
        if !paths.is_empty() {
            change_files(
                slot,
                &composers,
                &fetcher,
                FileChange::Attach { paths, start, end },
            )?;
        }
        Ok(view(slot))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn submitted(
    slot: &mut Slot,
    composers: &Composers,
    detail: Value,
    recover: bool,
    call: &impl Fn(&TaskCall) -> Result<serde_json::Map<String, Value>, String>,
) -> Result<bool, String> {
    if slot.blocked {
        return Err(zerus_client_core::drafts::NOT_RESTORED.into());
    }
    let key = slot.key();
    let store = &composers.0.store;
    writable(&detail, slot.mode)?;
    if slot.mode == Mode::Description && recover {
        return Err("Open this task's description to review the saved document.".into());
    }
    let original = if slot.mode == Mode::Note {
        let pending = outbox::pending(store.root(), &slot.project, &slot.task)?;
        if !recover && pending.is_none() && (slot.stored.uncertain || slot.stored.sending) {
            return Err(
                "The original message recovery snapshot is unavailable. This check sent nothing."
                    .into(),
            );
        }
        if !recover && pending.is_some() {
            slot.recovery = true;
            return Err("This task already has a message operation in progress.".into());
        }
        if recover {
            Some(pending.ok_or(
                "The original message recovery snapshot is unavailable. This check sent nothing.",
            )?)
        } else {
            None
        }
    } else {
        None
    };
    if !recover
        && slot.stored.draft.text.trim().is_empty()
        && slot.stored.draft.attachments.is_empty()
        && slot.mode == Mode::Note
    {
        return Err("Write a note for this task…".into());
    }
    let current_draft = slot.stored.draft.clone();
    let mut expected = current_draft.clone();
    slot.stored.sending = true;
    checkpoint(slot, store)?;
    let mut delivered = None;
    let result = (|| {
        if slot.mode == Mode::Description {
            // An interrupted replacement is checked against the canonical
            // document before another write; it is never replayed by a read.
            if slot.stored.uncertain {
                let current_files = tasks_write::decode_files(&detail["attachments"]);
                if detail["text"].as_str() == Some(expected.text.as_str())
                    && current_files == expected.attachments
                {
                    return Ok(());
                }
                return Err("The description save could not be confirmed. Review the task before saving another description.".into());
            }
            let reply = call(&TaskCall::edit(
                &slot.project,
                &slot.task,
                &expected.text,
                Some(&expected.attachments),
            ))?;
            if reply.get("task").and_then(|t| t["uuid"].as_str()) != Some(slot.task.as_str())
                || reply.get("task").and_then(|t| t["project"].as_str())
                    != Some(slot.project.as_str())
            {
                return Err("The task reply did not match the selected task.".into());
            }
        } else {
            let record = if let Some(record) = original {
                let content = &record["intent"]["content"];
                expected.text = content["text"].as_str().unwrap_or("").into();
                expected.attachments = tasks_write::decode_files(&content["attachments"]);
                record
            } else {
                let intent = tasks_write::message_intent(
                    &expected.text,
                    &expected.attachments,
                    "note",
                    detail["dispatch_id"].as_str().unwrap_or(""),
                );
                let reservation = outbox::reserve(
                    store.root(),
                    &slot.project,
                    &slot.task,
                    &intent,
                    &uuid::Uuid::new_v4().to_string(),
                )?;
                if reservation.recovering {
                    return Err("This task already has a message operation in progress.".into());
                }
                reservation.record
            };
            let reply = call(&tasks_write::message_call(
                &slot.project,
                &slot.task,
                &record,
                recover,
            ))?;
            tasks_write::message_delivered(
                &reply,
                &slot.task,
                record["id"].as_str().unwrap_or(""),
            )?;
            if reply.get("task").and_then(|t| t["project"].as_str()) != Some(slot.project.as_str())
            {
                return Err("The task reply did not match the selected task.".into());
            }
            delivered = Some(record["id"].as_str().unwrap_or("").to_owned());
        }
        Ok(())
    })();
    if let Err(error) = result {
        slot.stored.sending = false;
        slot.stored.uncertain = true;
        let saved = checkpoint(slot, store);
        slot.recovery = slot.mode == Mode::Note
            && outbox::pending(store.root(), &slot.project, &slot.task)?.is_some();
        saved?;
        return Err(error);
    }
    let cleared = clear(store, &key, expected)?;
    if !cleared {
        let (tx, rx) = mpsc::channel();
        store.settle_if_unchanged(&key, current_draft, false, move |result| {
            let _ = tx.send(result);
        });
        let settled = rx
            .recv()
            .map_err(|_| "The draft could not be saved.".to_owned())??;
        if !settled {
            return Err(CHANGED.into());
        }
    }
    slot.stored = load(store, &key)?.draft;
    slot.unsaved = false;
    slot.rev += 1;
    // Keep the durable original until the draft settlement is durable too.
    if let Some(id) = delivered {
        outbox::release(store.root(), &slot.project, &slot.task, &id)?;
    }

    slot.recovery = false;
    slot.error.clear();
    Ok(cleared)
}
#[tauri::command]
pub async fn task_editor_submit(
    writes: State<'_, crate::task_details::TaskWrites>,
    state: State<'_, TaskEditor>,
    composers: State<'_, Composers>,
    fetcher: State<'_, Arc<Fetcher>>,
    handle: u64,
    rev: u64,
    recover: bool,
) -> Result<View, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let write_permit = writes.begin()?;
    let permit = state.begin()?;
    let editor = state.inner().clone();
    let composers = composers.inner().clone();
    let fetcher = Arc::clone(fetcher.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let _write_permit = write_permit;
        let _permit = permit;
        let mut guard = crate::composer::lock(&editor.0.slot);
        let slot = guard
            .as_mut()
            .filter(|s| s.handle == handle && s.rev == rev)
            .ok_or(CHANGED)?;
        let result = document(&slot.project, &slot.task).and_then(|detail| {
            submitted(slot, &composers, detail, recover, &|call| {
                tasks_write::run(&zerus_client_core::default_hgs(), call)
            })
        });
        let saved = match result {
            Ok(saved) => saved,
            Err(error) => {
                slot.error = error;
                false
            }
        };
        if slot.mode == Mode::Note {
            slot.recovery = outbox::pending(composers.0.store.root(), &slot.project, &slot.task)
                .is_ok_and(|p| p.is_some());
        }
        refresh_files(slot, &composers, &fetcher)?;
        let mut answer = view(slot);
        answer.saved = saved;
        Ok(answer)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    const ID: &str = "11111111-1111-4111-8111-111111111111";
    fn slot(mode: Mode) -> Slot {
        Slot {
            handle: 1,
            rev: 0,
            project: "example".into(),
            task: ID.into(),
            mode,
            stored: StoredDraft {
                draft: Draft {
                    text: "Original".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            files: Vec::new(),
            pins: None,
            unsaved: false,
            error: String::new(),
            recovery: false,
            blocked: false,
        }
    }
    fn detail(text: &str) -> Value {
        json!({"uuid":ID,"project":"example","stage":"todo","writable":true,"text":text,"attachments":[]})
    }
    fn receipt(id: &str) -> serde_json::Map<String, Value> {
        json!({"task":{"uuid":ID,"project":"example"},"message":{"request_id":id}})
            .as_object()
            .unwrap()
            .clone()
    }
    #[test]
    fn note_unknown_is_only_checked_by_original_request_and_never_resubmitted() {
        let root = crate::composer::tests::dir("task-note-recovery");
        let store = crate::composer::tests::drafts(&root);
        let (hub, _) = crate::composer::tests::hub_on(&store);
        let mut slot = slot(Mode::Note);
        let calls = RefCell::new(Vec::new());
        let fail = |call: &TaskCall| {
            calls.borrow_mut().push(call.clone());
            Err("unknown outcome".to_owned())
        };
        assert!(submitted(&mut slot, &hub, detail(""), false, &fail).is_err());
        let record = outbox::pending(store.root(), "example", ID)
            .unwrap()
            .unwrap();
        assert!(submitted(&mut slot, &hub, detail(""), false, &fail).is_err());
        assert_eq!(calls.borrow().len(), 1);
        let recover = |call: &TaskCall| {
            assert!(call.args.iter().any(|a| a == "--recover-only"));
            assert!(call
                .args
                .iter()
                .any(|a| a == record["id"].as_str().unwrap()));
            assert_eq!(
                serde_json::from_str::<Value>(&call.input).unwrap()["text"],
                "Original"
            );
            Ok(receipt(record["id"].as_str().unwrap()))
        };
        assert!(submitted(&mut slot, &hub, detail(""), true, &recover).unwrap());
        assert!(slot.stored.draft.text.is_empty());
        assert!(!slot.stored.uncertain);
        assert!(outbox::pending(store.root(), "example", ID)
            .unwrap()
            .is_none());
    }
    #[test]
    fn recovery_of_original_note_keeps_newer_text_and_clears_only_its_delivery_flags() {
        let root = crate::composer::tests::dir("task-note-newer");
        let store = crate::composer::tests::drafts(&root);
        let (hub, _) = crate::composer::tests::hub_on(&store);
        let mut slot = slot(Mode::Note);
        assert!(submitted(&mut slot, &hub, detail(""), false, &|_| Err(
            "unknown".into()
        ))
        .is_err());
        edited(
            &mut slot,
            Edit {
                handle: 1,
                rev: 1,
                text: "Newer note".into(),
                position: 10,
                anchor: 10,
            },
        )
        .unwrap();
        save(&store, &slot.key(), slot.stored.clone()).unwrap();
        let record = outbox::pending(store.root(), "example", ID)
            .unwrap()
            .unwrap();
        let recovered = submitted(&mut slot, &hub, detail(""), true, &|call| {
            assert!(call.args.iter().any(|a| a == "--recover-only"));
            assert_eq!(
                serde_json::from_str::<Value>(&call.input).unwrap()["text"],
                "Original"
            );
            Ok(receipt(record["id"].as_str().unwrap()))
        })
        .unwrap();
        assert!(!recovered);
        assert_eq!(slot.stored.draft.text, "Newer note");
        assert!(!slot.stored.sending && !slot.stored.uncertain);
        assert_eq!(
            load(&store, &slot.key()).unwrap().draft.draft.text,
            "Newer note"
        );
    }
    #[test]
    fn description_unknown_is_resolved_by_read_and_wrong_stage_never_writes() {
        let root = crate::composer::tests::dir("task-description-check");
        let store = crate::composer::tests::drafts(&root);
        let (hub, _) = crate::composer::tests::hub_on(&store);
        let mut slot = slot(Mode::Description);
        let calls = Cell::new(0);
        let fail = |_: &TaskCall| {
            calls.set(calls.get() + 1);
            Err("unknown".into())
        };
        assert!(submitted(&mut slot, &hub, detail("previous"), false, &fail).is_err());
        assert!(submitted(&mut slot, &hub, detail("previous"), false, &fail).is_err());
        assert_eq!(calls.get(), 1);
        assert!(submitted(&mut slot, &hub, detail("Original"), false, &fail).unwrap());
        assert_eq!(calls.get(), 1);
        let mut review = detail("");
        review["stage"] = json!("review");
        assert!(submitted(&mut slot, &hub, review, false, &fail).is_err());
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn delivery_evidence_is_kept_until_the_draft_clear_is_durable() {
        let root = crate::composer::tests::dir("task-note-clear-failure");
        let store = crate::composer::tests::drafts(&root);
        let (hub, _) = crate::composer::tests::hub_on(&store);
        let mut slot = slot(Mode::Note);
        let blocked = RefCell::new(None);
        let result = submitted(&mut slot, &hub, detail(""), false, &|call| {
            let key_dir = std::fs::read_dir(store.root())
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.is_dir() && p.file_name().unwrap().to_string_lossy().len() == 64)
                .unwrap();
            let backup = root.join("saved-draft");
            std::fs::rename(&key_dir, &backup).unwrap();
            std::fs::write(&key_dir, b"blocked").unwrap();
            *blocked.borrow_mut() = Some((key_dir, backup));
            let at = call.args.iter().position(|a| a == "--request-id").unwrap();
            Ok(receipt(&call.args[at + 1]))
        });
        assert!(result.is_err());
        assert!(outbox::pending(store.root(), "example", ID)
            .unwrap()
            .is_some());
        let (key_dir, backup) = blocked.into_inner().unwrap();
        std::fs::remove_file(&key_dir).unwrap();
        std::fs::rename(backup, key_dir).unwrap();
        assert!(submitted(&mut slot, &hub, detail(""), true, &|call| {
            assert!(call.args.iter().any(|a| a == "--recover-only"));
            let at = call.args.iter().position(|a| a == "--request-id").unwrap();
            Ok(receipt(&call.args[at + 1]))
        })
        .unwrap());
        assert!(outbox::pending(store.root(), "example", ID)
            .unwrap()
            .is_none());
    }
    #[test]
    fn unverified_legacy_note_attempt_never_gets_a_new_request() {
        let root = crate::composer::tests::dir("task-note-legacy-unknown");
        let store = crate::composer::tests::drafts(&root);
        let (hub, _) = crate::composer::tests::hub_on(&store);
        let mut slot = slot(Mode::Note);
        slot.stored.uncertain = true;
        assert!(submitted(&mut slot, &hub, detail(""), false, &|_| panic!(
            "an unknown note cannot be submitted again"
        ))
        .is_err());
        assert!(outbox::pending(store.root(), "example", ID)
            .unwrap()
            .is_none());
    }
    #[test]
    fn stale_editor_and_oversize_edits_preserve_the_original_draft() {
        let mut slot = slot(Mode::Note);
        assert!(edited(
            &mut slot,
            Edit {
                handle: 2,
                rev: 1,
                text: "wrong".into(),
                position: 0,
                anchor: 0
            }
        )
        .is_err());
        assert!(edited(
            &mut slot,
            Edit {
                handle: 1,
                rev: 1,
                text: "x".repeat(65537),
                position: 0,
                anchor: 0
            }
        )
        .is_err());
        assert_eq!(slot.stored.draft.text, "Original");
        edited(
            &mut slot,
            Edit {
                handle: 1,
                rev: 2,
                text: "🙂abc".into(),
                position: 99,
                anchor: 2,
            },
        )
        .unwrap();
        assert_eq!(slot.stored.position, 5);
        assert_eq!(slot.stored.anchor, 2);
        assert!(edited(
            &mut slot,
            Edit {
                handle: 1,
                rev: 1,
                text: "stale".into(),
                position: 0,
                anchor: 0
            }
        )
        .is_err());
        assert_eq!(slot.stored.draft.text, "🙂abc");
    }
}

#[cfg(test)]
mod open_tests {
    use super::*;
    use std::cell::Cell;
    const ID: &str = "11111111-1111-4111-8111-111111111111";
    #[test]
    fn imported_description_keeps_its_text_cursor_and_bytes_without_reseeding() {
        let root = crate::composer::tests::dir("task-imported-editor");
        let store = crate::composer::tests::drafts(&root);
        let (hub, _) = crate::composer::tests::hub_on(&store);
        let key = format!("task-description\nexample\n{ID}");
        let mut stored = StoredDraft {
            draft: Draft {
                text: "Imported 🙂 description".into(),
                ..Default::default()
            },
            position: 11,
            anchor: 3,
            ..Default::default()
        };
        stored
            .draft
            .add(
                "notes.txt",
                "text/plain",
                b"private snapshot".to_vec(),
                Selection { start: 0, end: 0 },
            )
            .unwrap();
        save(&store, &key, stored.clone()).unwrap();
        let editor = TaskEditor::default();
        let opened = open_slot(
            &editor,
            &hub,
            "example".into(),
            ID.into(),
            Mode::Description,
            |_, _| panic!("an imported description must not be reseeded"),
        )
        .unwrap();
        assert_eq!(opened.stored, stored);
        assert_eq!(opened.key(), key);
    }
    #[test]
    fn empty_description_seeds_once_from_the_canonical_document_and_notes_never_read_it() {
        let root = crate::composer::tests::dir("task-seeded-editor");
        let store = crate::composer::tests::drafts(&root);
        let (hub, _) = crate::composer::tests::hub_on(&store);
        let editor = TaskEditor::default();
        let calls = Cell::new(0);
        let read = |_: &str, _: &str| {
            calls.set(calls.get() + 1);
            Ok(json!({"text":"Canonical", "stage":"todo", "attachments":[], "writable":true}))
        };
        let opened = open_slot(
            &editor,
            &hub,
            "example".into(),
            ID.into(),
            Mode::Description,
            read,
        )
        .unwrap();
        assert_eq!(opened.stored.draft.text, "Canonical");
        assert_eq!(calls.get(), 1);
        let second = open_slot(
            &editor,
            &hub,
            "example".into(),
            ID.into(),
            Mode::Description,
            read,
        )
        .unwrap();
        assert_eq!(second.stored.draft.text, "Canonical");
        assert_eq!(calls.get(), 1);
        let key = format!("task-description\nexample\n{ID}");
        save(
            &store,
            &key,
            StoredDraft {
                sending: true,
                ..Default::default()
            },
        )
        .unwrap();
        let interrupted = open_slot(
            &editor,
            &hub,
            "example".into(),
            ID.into(),
            Mode::Description,
            |_, _| panic!("an interrupted empty replacement is not reseeded"),
        )
        .unwrap();
        assert!(interrupted.stored.draft.text.is_empty() && interrupted.stored.uncertain);
        let note = open_slot(
            &editor,
            &hub,
            "example".into(),
            ID.into(),
            Mode::Note,
            |_, _| panic!("opening a note does not read the task"),
        )
        .unwrap();
        assert!(note.stored.draft.text.is_empty());
    }
}
