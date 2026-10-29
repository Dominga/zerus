//! Saved drafts, ported from `MessageComposer::showSavedDrafts`
//! (`tray/src/MessageComposer.cpp:1230-1387`): the drafts of this computer's
//! store, Qt's ("From the previous Zerus app") and the copies the switch
//! import left in their source (its conflicts, `<drafts root>/.imported-v1.json`),
//! each in bounded pages; a draft's preview; Recover, which copies a Qt draft
//! into this store; and Restore in current session.
//!
//! Qt's store and the dev app's are only ever read: `drafts::read_index`,
//! `drafts::read_draft` and `DraftStore::recover`, which reads Qt's copy and
//! writes only this store. Every call here blocks (the store's writer or the
//! disk): the commands run them on `spawn_blocking`, never under the hub lock.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::State;
use zerus_client_core::drafts::{
    self, Cursor, ImportReport, IndexPage, Loaded, OwnerKind, Page, StoredDraft, Summary, MAX_PAGE,
};

use super::{Composer, Composers, Hub, DRAFT_KEYS};

/// The machine a draft of this computer names (`showSavedDrafts`, 1265).
pub const THIS_COMPUTER: &str = "this computer";
/// Recover without Qt's store.
pub const NO_QT_DRAFTS: &str = "No saved drafts from the Qt app.";
/// A draft this hub is sending, as Saved drafts shows it (1310-1313).
pub const MAY_HAVE_BEEN_SENT: &str =
    "This message may already have been sent. Check Activity or Terminal before sending again.";
/// Restore found the current composer loading, busy or gone: Qt's Restore
/// is disabled then (1321-1323), and its click returns (1360).
pub const RESTORE_LATER: &str =
    "The current draft cannot be replaced while it is loading, sending or waiting to send.";
/// Recover when the store's writer gave no answer.
const NOT_RECOVERED: &str = "Could not recover this draft.";
/// The import's conflicts: the copies it left in their source.
pub const KEPT_IN_QT: &str = "Kept in the previous Zerus app";
pub const KEPT_IN_DEV: &str = "Kept in the development app";
/// The import marker is read only up to this size.
const MARKER_BYTES: u64 = 16 * 1024 * 1024;

/// Which store a Saved drafts row comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftSource {
    /// The client's own store ("On this computer").
    Client,
    /// Qt's store, read-only ("From the previous Zerus app").
    Qt,
    /// The dev app's stores, read-only: only the import's conflicts name them.
    Dev,
}

/// One Saved drafts row, as `showSavedDrafts` titles it (1257-1300; " / "
/// joins, never dots).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SavedDraftItem {
    pub key: String,
    pub kind: OwnerKind,
    /// The key's machine: a host alias, empty for this computer; a subagent
    /// key names the machine instead (`ComposerDraftOwner`).
    pub computer: String,
    pub session: String,
    /// The row's accessible text: the owner's parts joined by " / ".
    pub title: String,
    /// What the row shows as its title (Qt's `SessionRoles::Title`).
    pub caption: String,
    pub machine: String,
    /// The session and the draft's kind, archive and attachments.
    pub meta: String,
    /// The first line, or the attachment count.
    pub detail: String,
    /// Seconds since the epoch; 0 when unknown.
    pub modified: f64,
    pub attachments: usize,
    pub source: DraftSource,
    /// An archived session's draft: Go to session never reaches it
    /// (`SessionsWindow::draftOwner`).
    pub archived: bool,
}

/// One page of a Saved drafts section, newest first. `next` is an opaque
/// token for the following page: a cursor's nanoseconds exceed what a
/// JavaScript number holds exactly.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SavedDraftsPage {
    pub items: Vec<SavedDraftItem>,
    pub next: Option<String>,
}

/// One page of the import's conflicts; `next` is the offset of the next one.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ConflictsPage {
    pub items: Vec<SavedDraftItem>,
    pub next: Option<usize>,
}

/// The selected draft's preview (`presentSelection`, 1309-1324).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SavedDraftDetail {
    pub text: String,
    pub attachments: usize,
    pub detail: String,
    /// No storage error and no voice message (1321-1323).
    pub restorable: bool,
}

/// `%n attachment(s)`.
fn attachment_count(count: usize) -> String {
    if count == 1 {
        "1 attachment".to_owned()
    } else {
        format!("{count} attachments")
    }
}

/// The token of a page cursor.
pub fn cursor_token(cursor: &Cursor) -> String {
    format!("{}:{}", cursor.modified_ns, cursor.folder)
}

/// The cursor a token names.
pub fn cursor_of(token: &str) -> Option<Cursor> {
    let (nanos, folder) = token.split_once(':')?;
    Some(Cursor {
        modified_ns: nanos.parse().ok()?,
        folder: folder.to_owned(),
    })
}

/// One Saved drafts row (`showSavedDrafts`, 1257-1300).
pub(crate) fn item(key: String, summary: &Summary, source: DraftSource) -> SavedDraftItem {
    let owner = drafts::owner(&key);
    let machine = if owner.machine.is_empty() {
        THIS_COMPUTER.to_owned()
    } else {
        owner.machine.clone()
    };
    let mut title = Vec::new();
    match owner.kind {
        OwnerKind::TaskDescription | OwnerKind::TaskComment => {
            title.push(if summary.label.is_empty() {
                owner.task.clone()
            } else {
                summary.label.clone()
            });
            title.push(owner.project.clone());
        }
        OwnerKind::Unknown => title.push(if summary.label.is_empty() {
            key.replace('\n', " / ")
        } else {
            summary.label.clone()
        }),
        kind => {
            title.push(owner.session.clone());
            title.push(machine.clone());
            match kind {
                OwnerKind::Subagent => title.push("subagent".into()),
                OwnerKind::Question => title.push("question answer".into()),
                OwnerKind::Voice => title.push("voice message".into()),
                OwnerKind::Task => title.push("task prompt".into()),
                _ => {}
            }
            if !owner.archive.is_empty() {
                title.push("archived".into());
            }
        }
    }
    // `section('/', 2)` is everything after the second slash.
    let mut caption = owner
        .session
        .splitn(3, '/')
        .nth(2)
        .filter(|c| !c.is_empty())
        .or_else(|| owner.session.split('/').nth(1).filter(|c| !c.is_empty()))
        .map(str::to_owned)
        .unwrap_or_else(|| title[0].clone());
    let mut context = Vec::new();
    match owner.kind {
        OwnerKind::Subagent => {
            context.push("Subagent".to_owned());
            caption = format!("{caption} / {}", owner.subagent);
        }
        OwnerKind::Question => context.push("Question answer".into()),
        OwnerKind::Voice => context.push("Voice message".into()),
        OwnerKind::Task => context.push("Task prompt".into()),
        _ => {}
    }
    if !owner.archive.is_empty() {
        context.push("Archived".into());
    }
    let count = attachment_count(summary.attachments);
    if summary.attachments > 0 && !summary.preview.is_empty() {
        context.push(count.clone());
    }
    // The session, and the context after it when there is any (1294).
    let meta = if context.is_empty() {
        owner.session.clone()
    } else {
        std::iter::once(owner.session.clone())
            .filter(|session| !session.is_empty())
            .chain(context)
            .collect::<Vec<_>>()
            .join(" / ")
    };
    SavedDraftItem {
        kind: owner.kind,
        computer: owner.machine.clone(),
        session: owner.session.clone(),
        title: title.join(" / "),
        caption,
        machine,
        meta,
        detail: if summary.preview.is_empty() && summary.attachments > 0 {
            count
        } else {
            summary.preview.clone()
        },
        modified: summary.modified,
        attachments: summary.attachments,
        source,
        archived: !owner.archive.is_empty(),
        key,
    }
}

/// A draft as Saved drafts shows it: its content, and its delivery doubt.
#[derive(Default)]
struct Selected {
    stored: StoredDraft,
    uncertain: bool,
    error: bool,
    notice: String,
    storage_error: Option<String>,
}

impl From<Loaded> for Selected {
    /// A draft read from a disk (Qt's `loadDirect`, `ComposerDraftStore.cpp:178-180`):
    /// one saved while sending reads as uncertain, with the interrupted notice.
    fn from(loaded: Loaded) -> Self {
        let mut stored = loaded.draft;
        let uncertain = stored.uncertain || stored.sending;
        stored.sending = false;
        stored.uncertain = uncertain;
        Selected {
            stored,
            uncertain,
            error: uncertain,
            notice: loaded.notice.unwrap_or_default(),
            storage_error: loaded.storage_error,
        }
    }
}

impl Selected {
    /// The voice message's session: Restore never takes a voice draft (1318-1323).
    fn voice_session(&self) -> Option<String> {
        let form = &self.stored.form_state;
        (form.get("kind").and_then(Value::as_str) == Some("voice_message")).then(|| {
            form.get("session")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
    }
}

fn voice_text(session: &str) -> String {
    format!("Open the original conversation ({session}) to review or check this voice message.")
}

/// Reads at most `limit` bytes, failing when the file is larger.
fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other("too large"));
    }
    Ok(bytes)
}

/// The import's conflicts in its marker, in their order, each once: the
/// copies it left in Qt's or the dev app's store.
fn read_conflicts(marker: &Path) -> Vec<(String, DraftSource)> {
    #[derive(Deserialize)]
    struct Marker {
        report: ImportReport,
    }
    let Some(marker) = read_bounded(marker, MARKER_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok())
    else {
        return Vec::new();
    };
    let mut found: Vec<(String, DraftSource)> = marker
        .report
        .conflicts
        .into_iter()
        .filter_map(|conflict| {
            let source = match conflict.source.as_str() {
                "qt" => DraftSource::Qt,
                "dev" => DraftSource::Dev,
                _ => return None,
            };
            Some((conflict.key, source))
        })
        .collect();
    found.dedup();
    found
}

/// A draft of the dev app's stores: the first valid copy with content,
/// else the first damaged one.
fn read_dev(roots: &[PathBuf], key: &str) -> Loaded {
    let mut damaged = None;
    for root in roots {
        let found = drafts::read_draft(root, key);
        if found.storage_error.is_some() {
            damaged.get_or_insert(found);
        } else if !found.draft.draft.text.is_empty() || !found.draft.draft.attachments.is_empty() {
            return found;
        }
    }
    damaged.unwrap_or_default()
}

impl Composers {
    /// One page of a Saved drafts section, at most 200 drafts older than
    /// `before`. Blocking: this store's page waits for its writer, after
    /// every save queued before it; Qt's is read from its folder.
    pub fn saved_drafts(
        &self,
        source: DraftSource,
        limit: usize,
        before: Option<Cursor>,
    ) -> SavedDraftsPage {
        let page = IndexPage {
            limit: limit.clamp(1, MAX_PAGE),
            before,
        };
        let found = match source {
            DraftSource::Client => {
                let (sender, answer) = mpsc::channel();
                self.0.store.index(page, move |found| {
                    let _ = sender.send(found);
                });
                answer.recv().unwrap_or_default()
            }
            DraftSource::Qt => self
                .0
                .qt
                .as_deref()
                .map(|root| drafts::read_index(root, &page))
                .unwrap_or_default(),
            // Only the import's conflicts name the dev app's drafts.
            DraftSource::Dev => Page::default(),
        };
        SavedDraftsPage {
            items: found
                .items
                .into_iter()
                .map(|(key, summary)| item(key, &summary, source))
                .collect(),
            next: found.next.as_ref().map(cursor_token),
        }
    }

    /// One page of the switch import's conflicts (zerus-5wmh.13.1): the
    /// copies it did not write because a different draft was here, in its
    /// marker's order. Blocking: it reads the marker (at most 16 MiB).
    pub fn import_conflicts(&self, limit: usize, after: usize) -> ConflictsPage {
        let conflicts = read_conflicts(&self.0.store.root().join(drafts::IMPORT_MARKER));
        let limit = limit.clamp(1, MAX_PAGE);
        let empty = Summary {
            label: String::new(),
            preview: String::new(),
            modified: 0.0,
            attachments: 0,
        };
        ConflictsPage {
            items: conflicts
                .iter()
                .skip(after)
                .take(limit)
                .map(|(key, source)| SavedDraftItem {
                    detail: if *source == DraftSource::Qt {
                        KEPT_IN_QT
                    } else {
                        KEPT_IN_DEV
                    }
                    .into(),
                    ..item(key.clone(), &empty, *source)
                })
                .collect(),
            next: (after.saturating_add(limit) < conflicts.len()).then_some(after + limit),
        }
    }

    /// The selected draft's text and detail line (`presentSelection`).
    /// Blocking: a draft this hub does not hold is read from its store.
    pub fn saved_draft(&self, source: DraftSource, key: &str) -> SavedDraftDetail {
        let selected = self.selected(source, key);
        let voice = selected.voice_session();
        let attachments = selected.stored.draft.attachments.len();
        let detail = if let Some(error) = &selected.storage_error {
            error.clone()
        } else if let Some(session) = &voice {
            voice_text(session)
        } else {
            std::iter::once(attachment_count(attachments))
                .chain((!selected.notice.is_empty()).then(|| selected.notice.clone()))
                .collect::<Vec<_>>()
                .join(" / ")
        };
        SavedDraftDetail {
            restorable: selected.storage_error.is_none() && voice.is_none(),
            text: selected.stored.draft.text,
            attachments,
            detail,
        }
    }

    fn selected(&self, source: DraftSource, key: &str) -> Selected {
        match source {
            DraftSource::Client => {
                if let Some(held) = self.held(key) {
                    return held;
                }
                let (sender, answer) = mpsc::channel();
                self.0.store.load(key, move |loaded| {
                    let _ = sender.send(loaded);
                });
                Selected::from(answer.recv().unwrap_or_default())
            }
            DraftSource::Qt => Selected::from(
                self.0
                    .qt
                    .as_deref()
                    .map(|root| drafts::read_draft(root, key))
                    .unwrap_or_default(),
            ),
            DraftSource::Dev => Selected::from(read_dev(&self.0.dev, key)),
        }
    }

    /// A draft this hub holds loaded, newer than its stored copy (Qt reads
    /// `m_preservedDrafts`, then `m_drafts`, before the store: 1331-1333).
    /// A draft being sent may have been delivered; a Retry that keeps the
    /// draft shows the draft as it was.
    fn held(&self, key: &str) -> Option<Selected> {
        let hub = self.lock();
        let composer = hub.composers.get(key).filter(|c| c.loaded && !c.renaming)?;
        let (sending, error, notice) = match &composer.kept {
            Some((error, notice)) => (false, *error, notice.clone()),
            None => (composer.sending, composer.error, composer.notice.clone()),
        };
        let uncertain = composer.uncertain || sending;
        let mut stored = composer.stored();
        stored.sending = false;
        stored.uncertain = uncertain;
        Some(Selected {
            stored,
            uncertain,
            error,
            notice: if sending {
                MAY_HAVE_BEEN_SENT.to_owned()
            } else if error || uncertain {
                notice
            } else {
                String::new()
            },
            storage_error: composer.storage_error.clone(),
        })
    }

    /// Recover: copies Qt's draft of `key` into this store, never over a
    /// draft here (`DraftStore::recover`'s `ALREADY_EXISTS`). Blocking.
    pub fn recover(&self, key: &str) -> Result<(), String> {
        let root = self.0.qt.clone().ok_or_else(|| NO_QT_DRAFTS.to_owned())?;
        self.recover_from(&root, key)
    }

    fn recover_from(&self, root: &Path, key: &str) -> Result<(), String> {
        let (sender, answer) = mpsc::channel();
        let me = self.clone();
        let owned = key.to_owned();
        self.0.store.recover(root, key, move |result| {
            // A copy that could not be read completely is not written.
            let result = result.and_then(|copy| match copy.storage_error {
                Some(error) => Err(error),
                None => {
                    me.recovered(&owned, copy);
                    Ok(())
                }
            });
            let _ = sender.send(result);
        });
        answer
            .recv()
            .unwrap_or_else(|_| Err(NOT_RECOVERED.to_owned()))
    }

    /// A draft recovered into this store (on its writer). A composer held
    /// here with no draft of its own takes it, as its load would, and saves
    /// it again, so a save of its empty draft queued meanwhile cannot leave
    /// the store empty; the cards' draft mark follows.
    fn recovered(&self, key: &str, copy: Loaded) {
        if copy.draft.draft.text.is_empty() && copy.draft.draft.attachments.is_empty() {
            return;
        }
        let mut hub = self.lock();
        if !hub.stored_keys.iter().any(|k| k == key) {
            hub.stored_keys.push_front(key.to_owned());
            hub.stored_keys.truncate(DRAFT_KEYS);
        }
        let empty = |c: &&mut Composer| {
            c.loaded
                && !c.busy()
                && c.storage_error.is_none()
                && c.draft.text.is_empty()
                && c.draft.attachments.is_empty()
        };
        if let Some(composer) = hub.composers.get_mut(key).filter(empty) {
            let stored = copy.draft;
            composer.draft = stored.draft;
            composer.position = stored.position;
            composer.anchor = stored.anchor;
            composer.label = stored.label;
            composer.form_state = stored.form_state;
            composer.uncertain = stored.uncertain || stored.sending || copy.notice.is_some();
            if let Some(notice) = copy.notice {
                composer.show_error(notice);
            }
            composer.removed = None;
            composer.text_epoch += 1;
            composer.attachments_changed();
            self.save(composer);
        }
        drop(hub);
        self.changed();
    }

    /// The open composer `handle` while Restore may replace its draft:
    /// loaded, not sending, waiting, attaching or renaming, and the app not
    /// quitting (Qt's checks, 1321-1323 and 1360).
    fn restore_target(hub: &mut Hub, handle: u64) -> Option<&mut Composer> {
        let open = hub.open_key.clone()?;
        Self::editable(hub, handle).filter(|c| c.key == open)
    }

    /// Restore in current session: the open composer's draft becomes the
    /// selected one, its label and form state cleared, its delivery doubt
    /// kept (1355-1371). The webview asked "Replace current draft?" first.
    /// A Qt draft is also recovered into this store; a draft already there
    /// stays, and its `ALREADY_EXISTS` does not stop the restore. Blocking.
    pub fn restore(&self, handle: u64, source: DraftSource, key: &str) -> Result<(), String> {
        if Self::restore_target(&mut self.lock(), handle).is_none() {
            return Err(RESTORE_LATER.into());
        }
        let selected = self.selected(source, key);
        if let Some(error) = selected.storage_error.clone() {
            return Err(error);
        }
        if let Some(session) = selected.voice_session() {
            return Err(voice_text(&session));
        }
        if source == DraftSource::Qt {
            if let Some(root) = self.0.qt.clone() {
                let _ = self.recover_from(&root, key);
            }
        }
        let mut hub = self.lock();
        let Some(composer) = Self::restore_target(&mut hub, handle) else {
            return Err(RESTORE_LATER.into());
        };
        let Selected {
            stored,
            uncertain,
            error,
            notice,
            ..
        } = selected;
        composer.draft = stored.draft;
        composer.position = stored.position;
        composer.anchor = stored.anchor;
        composer.label.clear();
        composer.form_state = Map::new();
        composer.uncertain = uncertain;
        composer.error = error;
        composer.notice = notice;
        composer.removed = None;
        composer.text_epoch += 1;
        composer.attachments_changed();
        self.save(composer);
        drop(hub);
        self.changed();
        Ok(())
    }
}

/// One page of a Saved drafts section; `before` is the previous page's `next`.
#[tauri::command]
pub async fn saved_drafts(
    state: State<'_, Composers>,
    source: DraftSource,
    limit: usize,
    before: Option<String>,
) -> Result<SavedDraftsPage, String> {
    let before = match before {
        Some(token) => Some(cursor_of(&token).ok_or("Invalid Saved drafts page.")?),
        None => None,
    };
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.saved_drafts(source, limit, before))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn draft_conflicts(
    state: State<'_, Composers>,
    limit: usize,
    after: usize,
) -> Result<ConflictsPage, String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.import_conflicts(limit, after))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn saved_draft(
    state: State<'_, Composers>,
    source: DraftSource,
    key: String,
) -> Result<SavedDraftDetail, String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.saved_draft(source, &key))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn draft_recover(state: State<'_, Composers>, key: String) -> Result<(), String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.recover(&key))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn draft_restore(
    state: State<'_, Composers>,
    handle: u64,
    source: DraftSource,
    key: String,
) -> Result<(), String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.restore(handle, source, &key))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::qt_store;
    use crate::composer::tests::{
        dir, drafts, hub, hub_with_qt, hub_with_stores, session, until, Scratch, KEY,
    };
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::sync::Arc;
    use zerus_client_core::drafts::{read_draft, DraftStore, ImportConflict};

    /// A Qt store with one draft, written by a store of its own that is gone
    /// before the client's hub opens.
    fn qt_store_with_a_draft(root: &Path) -> PathBuf {
        let qt = root.join("qt");
        let writer = DraftStore::open(qt.clone());
        let mut stored = StoredDraft {
            sending: true,
            ..StoredDraft::default()
        };
        stored.draft.text = "from the Qt app".into();
        writer.save_durable(KEY, stored, |_| {});
        writer.flush();
        drop(writer);
        qt
    }

    /// Writes `drafts` into the store at `root` (each a key and its stored
    /// draft) with a store of its own, gone before the test goes on.
    fn write_store(root: PathBuf, drafts: Vec<(&str, StoredDraft)>) -> PathBuf {
        let writer = DraftStore::open(root.clone());
        for (key, draft) in drafts {
            writer.save_durable(key, draft, |_| {});
        }
        writer.flush();
        drop(writer);
        root
    }

    fn text(text: &str) -> StoredDraft {
        let mut stored = StoredDraft::default();
        stored.draft.text = text.into();
        stored
    }

    /// Every file under `root`, with its bytes and modification time.
    fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        let mut files = Vec::new();
        let mut folders = vec![root.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                    continue;
                }
                let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
                files.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(&path).unwrap(),
                    modified,
                ));
            }
        }
        files.sort();
        files
    }

    #[test]
    fn saved_drafts_page_and_name_their_owner() {
        let root = dir("saved");
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(
            view.handle,
            1,
            view.text_epoch,
            "first line\nsecond".into(),
            0,
            0,
        );
        // The index runs on the store's writer after the queued save.
        let page = c.saved_drafts(DraftSource::Client, 10, None);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.next, None);
        let item = &page.items[0];
        assert_eq!(item.title, "claude/sample-project/review / studio");
        assert_eq!(item.caption, "review");
        assert_eq!(item.machine, "studio");
        assert_eq!(item.meta, "claude/sample-project/review");
        assert_eq!(item.detail, "first line");
        assert_eq!(
            (item.kind, item.source, item.archived),
            (OwnerKind::Message, DraftSource::Client, false)
        );
        let detail = c.saved_draft(DraftSource::Client, KEY);
        assert_eq!(
            (detail.text.as_str(), detail.detail.as_str()),
            ("first line\nsecond", "0 attachments")
        );
        assert!(detail.restorable);
    }

    #[test]
    fn a_qt_draft_is_recovered_never_over_a_draft_here() {
        let root = dir("recover");
        let qt = qt_store_with_a_draft(&root);
        let (c, _) = hub_with_qt(&drafts(&root), Some(qt));
        let page = c.saved_drafts(DraftSource::Qt, 10, None);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].source, DraftSource::Qt);
        assert_eq!(c.recover(KEY), Ok(()));
        assert_eq!(
            c.saved_draft(DraftSource::Client, KEY).text,
            "from the Qt app"
        );
        assert_eq!(
            c.recover(KEY),
            Err(
                "Another draft already exists here. Recover the previous one from Saved drafts."
                    .into()
            )
        );
        // The cards mark the recovered draft.
        assert!(c.take_draft_keys().unwrap().contains(&KEY.to_owned()));
    }

    #[test]
    fn recover_without_qts_store_says_so() {
        let root = dir("recover-none");
        let (c, _) = hub(&root);
        assert_eq!(c.recover(KEY), Err(NO_QT_DRAFTS.into()));
        assert!(c.saved_drafts(DraftSource::Qt, 10, None).items.is_empty());
    }

    #[test]
    fn the_qt_store_is_never_written() {
        let root = dir("qt-untouched");
        let qt = qt_store_with_a_draft(&root);
        let before = snapshot(&qt);
        let store = drafts(&root);
        let (c, _) = hub_with_qt(&store, Some(qt.clone()));
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        assert_eq!(c.saved_drafts(DraftSource::Qt, 10, None).items.len(), 1);
        assert!(c
            .saved_draft(DraftSource::Qt, KEY)
            .text
            .contains("from the Qt app"));
        assert_eq!(c.recover(KEY), Ok(()));
        assert!(c.recover(KEY).is_err());
        assert_eq!(c.restore(view.handle, DraftSource::Qt, KEY), Ok(()));
        c.flush();
        store.flush();
        assert_eq!(
            snapshot(&qt),
            before,
            "Qt's folder is byte-for-byte unchanged, with no file added or removed"
        );
    }

    #[test]
    fn the_qt_store_is_never_the_clients_own() {
        let root = dir("qt-same");
        let own = root.join("drafts");
        std::fs::create_dir_all(&own).unwrap();
        assert_eq!(qt_store(&own, Some(own.clone())), None);
        assert_eq!(
            qt_store(&own, Some(root.to_path_buf())),
            None,
            "nor a folder that holds it"
        );
        assert_eq!(
            qt_store(&own, Some(own.join("inner"))),
            None,
            "nor a folder inside it"
        );
        assert_eq!(qt_store(&own, Some(root.join("qt"))), Some(root.join("qt")));
        assert_eq!(qt_store(&own, None), None);
        // The hub applies the rule too: Qt's "store" at its own root lists nothing.
        let store = Arc::new(DraftStore::open(own.clone()));
        let (c, _) = hub_with_qt(&store, Some(own.clone()));
        assert_eq!(c.recover(KEY), Err(NO_QT_DRAFTS.into()));
        drop(c);
        store.flush();
    }

    #[test]
    fn a_recovered_draft_reaches_the_empty_composer_held_here() {
        let root = dir("recover-held");
        let qt = qt_store_with_a_draft(&root);
        let store = drafts(&root);
        let (c, _) = hub_with_qt(&store, Some(qt));
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        assert_eq!(view.text, "");
        assert_eq!(c.recover(KEY), Ok(()));
        let view = until(&c, |v| v.text == "from the Qt app");
        // Saved while sending in Qt: it may have been delivered.
        assert!(view.uncertain && !view.can_send);
        c.flush();
        store.flush();
        assert_eq!(
            read_draft(&root.join("drafts"), KEY).draft.draft.text,
            "from the Qt app",
            "the held composer saves what it took, never its empty draft"
        );
    }

    #[test]
    fn restore_replaces_the_open_draft_and_keeps_its_doubt() {
        let root = dir("restore");
        let other = "\nclaude/sample-project/other";
        let mut saved = text("saved elsewhere\nline two");
        saved.uncertain = true;
        saved.label = "named".into();
        saved.form_state.insert("title".into(), json!("a form"));
        let store_root = write_store(root.join("drafts"), vec![(other, saved)]);
        let store = Arc::new(DraftStore::open(store_root.clone()));
        let (c, _) = crate::composer::tests::hub_on(&store);
        // A composer held but no longer open.
        c.open(Some(crate::composer::send::tests::other_session()));
        let left = until(&c, |v| v.loaded);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded && v.handle != left.handle);
        c.edit(view.handle, 1, view.text_epoch, "current".into(), 0, 0);
        assert_eq!(
            c.restore(left.handle, DraftSource::Client, other),
            Err(RESTORE_LATER.into()),
            "only the open composer's draft is replaced"
        );
        assert_eq!(c.current().unwrap().text, "current");
        assert_eq!(c.restore(view.handle, DraftSource::Client, other), Ok(()));
        let restored = c.current().unwrap();
        assert_eq!(restored.text, "saved elsewhere\nline two");
        assert!(restored.text_epoch > view.text_epoch);
        assert!(
            restored.uncertain && !restored.can_send,
            "a draft that may have been sent stays uncertain until Allow retry"
        );
        assert_eq!(
            restored.status,
            "Delivery was interrupted. Check Activity or Terminal before sending again."
        );
        c.flush();
        store.flush();
        let written = read_draft(&store_root, KEY).draft;
        assert_eq!(written.draft.text, "saved elsewhere\nline two");
        assert!(written.label.is_empty() && written.form_state.is_empty());
        assert_eq!(
            read_draft(&store_root, other).draft.draft.text,
            "saved elsewhere\nline two",
            "the saved draft stays where it was"
        );
        drop(c);
        drop(store);
    }

    #[test]
    fn restore_waits_for_an_idle_composer() {
        let root = dir("restore-busy");
        let other = "\nclaude/sample-project/other";
        let store_root = write_store(root.join("drafts"), vec![(other, text("saved"))]);
        let store = Arc::new(DraftStore::open(store_root));
        let (c, _) = crate::composer::tests::hub_on(&store);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        crate::composer::lock(&c.0.hub)
            .composers
            .get_mut(KEY)
            .unwrap()
            .sending = true;
        assert_eq!(
            c.restore(view.handle, DraftSource::Client, other),
            Err(RESTORE_LATER.into())
        );
        crate::composer::lock(&c.0.hub)
            .composers
            .get_mut(KEY)
            .unwrap()
            .sending = false;
        assert_eq!(c.restore(view.handle, DraftSource::Client, other), Ok(()));
        drop(c);
        store.flush();
    }

    #[test]
    fn a_voice_draft_is_never_restored() {
        let root = dir("restore-voice");
        let key = "voice-message-draft\n\nclaude/sample-project/review";
        let mut voice = text("recognized words");
        voice
            .form_state
            .insert("kind".into(), json!("voice_message"));
        voice
            .form_state
            .insert("session".into(), json!("claude/sample-project/review"));
        let store_root = write_store(root.join("drafts"), vec![(key, voice)]);
        let store = Arc::new(DraftStore::open(store_root));
        let (c, _) = crate::composer::tests::hub_on(&store);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        let page = c.saved_drafts(DraftSource::Client, 10, None);
        assert_eq!(
            page.items[0].title,
            "claude/sample-project/review / this computer / voice message"
        );
        assert_eq!(
            page.items[0].meta,
            "claude/sample-project/review / Voice message"
        );
        let detail = c.saved_draft(DraftSource::Client, key);
        assert_eq!(
            detail.detail,
            "Open the original conversation (claude/sample-project/review) to review or check this voice message."
        );
        assert!(!detail.restorable);
        assert!(c.restore(view.handle, DraftSource::Client, key).is_err());
        assert_eq!(c.current().unwrap().text, "");
        drop(c);
        store.flush();
    }

    #[test]
    fn a_draft_being_sent_may_already_be_delivered() {
        let root = dir("saved-sending");
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.edit(view.handle, 1, view.text_epoch, "on its way".into(), 0, 0);
        crate::composer::lock(&c.0.hub)
            .composers
            .get_mut(KEY)
            .unwrap()
            .sending = true;
        let detail = c.saved_draft(DraftSource::Client, KEY);
        assert_eq!(detail.text, "on its way");
        assert_eq!(
            detail.detail,
            "0 attachments / This message may already have been sent. Check Activity or Terminal before sending again."
        );
        // A Retry that keeps the draft shows the draft as it was.
        {
            let mut hub = crate::composer::lock(&c.0.hub);
            let composer = hub.composers.get_mut(KEY).unwrap();
            composer.kept = Some((false, String::new()));
        }
        assert_eq!(
            c.saved_draft(DraftSource::Client, KEY).detail,
            "0 attachments"
        );
    }

    #[test]
    fn pages_never_repeat_or_skip_a_draft() {
        let root = dir("saved-pages");
        let keys: Vec<String> = (0..5)
            .map(|n| format!("\nclaude/sample-project/tag-{n}"))
            .collect();
        let store_root = write_store(
            root.join("drafts"),
            keys.iter().map(|k| (k.as_str(), text("x"))).collect(),
        );
        let store = Arc::new(DraftStore::open(store_root));
        let (c, _) = crate::composer::tests::hub_on(&store);
        let mut seen = Vec::new();
        let mut before = None;
        loop {
            let page = c.saved_drafts(DraftSource::Client, 2, before);
            assert!(page.items.len() <= 2);
            seen.extend(page.items.into_iter().map(|item| item.key));
            match page.next {
                Some(token) => before = Some(cursor_of(&token).unwrap()),
                None => break,
            }
        }
        seen.sort();
        let mut expected = keys.clone();
        expected.sort();
        assert_eq!(seen, expected);
        assert_eq!(
            c.saved_drafts(DraftSource::Client, 0, None).items.len(),
            1,
            "a page holds at least one draft"
        );
        drop(c);
        store.flush();
    }

    #[test]
    fn cursor_tokens_keep_every_nanosecond() {
        let cursor = Cursor {
            modified_ns: 1_793_000_000_123_456_789,
            folder: "ab".repeat(32),
        };
        assert_eq!(cursor_of(&cursor_token(&cursor)), Some(cursor));
        assert_eq!(cursor_of("not a cursor"), None);
        assert_eq!(
            cursor_of("12:"),
            Some(Cursor {
                modified_ns: 12,
                folder: String::new()
            })
        );
    }

    #[test]
    fn items_follow_qts_titles() {
        let summary = |preview: &str, attachments: usize| Summary {
            label: String::new(),
            preview: preview.into(),
            modified: 1.0,
            attachments,
        };
        let subagent = item(
            "workstation\nclaude/p/a\nconv-1\nagent-2".into(),
            &summary("hi", 2),
            DraftSource::Qt,
        );
        assert_eq!(subagent.title, "claude/p/a / workstation / subagent");
        assert_eq!(subagent.caption, "a / agent-2");
        assert_eq!(subagent.meta, "claude/p/a / Subagent / 2 attachments");
        assert_eq!(subagent.detail, "hi");
        let archived = item(
            "\nclaude/p/a\narchive-7".into(),
            &summary("", 1),
            DraftSource::Client,
        );
        assert_eq!(archived.title, "claude/p/a / this computer / archived");
        assert_eq!(archived.meta, "claude/p/a / Archived");
        assert_eq!(archived.detail, "1 attachment");
        assert!(archived.archived);
        let task = item(
            "task-comment\nalpha\nt-1".into(),
            &summary("note", 0),
            DraftSource::Client,
        );
        assert_eq!(
            (task.title.as_str(), task.caption.as_str()),
            ("t-1 / alpha", "t-1")
        );
        assert_eq!(task.meta, "");
        let unknown = item("lonely".into(), &summary("x", 0), DraftSource::Client);
        assert_eq!(
            (unknown.title.as_str(), unknown.caption.as_str()),
            ("lonely", "lonely")
        );
        let deep = item(
            "\nclaude/p/review/extra".into(),
            &summary("x", 0),
            DraftSource::Client,
        );
        assert_eq!(
            deep.caption, "review/extra",
            "Qt's section('/', 2) keeps the rest"
        );
    }

    /// The marker the switch import writes, with these conflicts.
    fn marker(store: &Path, conflicts: &[(&str, &str)]) {
        let report = ImportReport {
            conflicts: conflicts
                .iter()
                .map(|(key, source)| ImportConflict {
                    key: (*key).into(),
                    source: (*source).into(),
                })
                .collect(),
            ..ImportReport::default()
        };
        std::fs::create_dir_all(store).unwrap();
        std::fs::write(
            store.join(drafts::IMPORT_MARKER),
            json!({"version": 1, "imported_at": 1, "report": report}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn the_import_conflicts_page_and_read_their_source() {
        let root = dir("conflicts");
        let qt = qt_store_with_a_draft(&root);
        let dev_key = "\nclaude/sample-project/dev";
        let dev = write_store(root.join("dev"), vec![(dev_key, text("from the dev app"))]);
        let qt_before = snapshot(&qt);
        let dev_before = snapshot(&dev);
        let store = drafts(&root);
        marker(
            &root.join("drafts"),
            &[
                (KEY, "qt"),
                (KEY, "qt"),
                (dev_key, "dev"),
                ("\nclaude/p/other", "elsewhere"),
            ],
        );
        let (c, _) = hub_with_stores(&store, Some(qt), vec![dev.clone()]);
        let first = c.import_conflicts(1, 0);
        assert_eq!(first.items.len(), 1);
        assert_eq!(
            (
                first.items[0].key.as_str(),
                first.items[0].source,
                first.items[0].detail.as_str()
            ),
            (KEY, DraftSource::Qt, KEPT_IN_QT)
        );
        assert_eq!(first.next, Some(1));
        let second = c.import_conflicts(1, 1);
        assert_eq!(
            (
                second.items[0].key.as_str(),
                second.items[0].source,
                second.items[0].detail.as_str()
            ),
            (dev_key, DraftSource::Dev, KEPT_IN_DEV)
        );
        assert_eq!(second.next, None);
        assert_eq!(c.import_conflicts(200, 0).items.len(), 2);
        assert_eq!(
            c.saved_draft(DraftSource::Dev, dev_key).text,
            "from the dev app"
        );
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        assert_eq!(c.restore(view.handle, DraftSource::Dev, dev_key), Ok(()));
        assert_eq!(c.current().unwrap().text, "from the dev app");
        c.flush();
        store.flush();
        assert_eq!(
            snapshot(&dev),
            dev_before,
            "the dev app's store is only read"
        );
        assert_eq!(snapshot(&root.join("qt")), qt_before);
    }

    #[test]
    fn without_a_marker_there_are_no_conflicts() {
        let root: Scratch = dir("conflicts-none");
        let (c, _) = hub(&root);
        assert_eq!(c.import_conflicts(10, 0), ConflictsPage::default());
        std::fs::create_dir_all(root.join("drafts")).unwrap();
        std::fs::write(root.join("drafts").join(drafts::IMPORT_MARKER), "{not json").unwrap();
        assert_eq!(c.import_conflicts(10, 0), ConflictsPage::default());
    }

    /// A draft folder whose manifest names another key: it cannot be read completely.
    fn damaged(root: &Path, key: &str) {
        let folder = root.join(
            Sha256::digest(key.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
        );
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("draft.json"),
            json!({"version": 1, "key": "another key", "text": "x"}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn a_damaged_draft_is_neither_restored_nor_recovered() {
        let root = dir("restore-damaged");
        let qt = root.join("qt");
        damaged(&qt, KEY);
        let qt_before = snapshot(&qt);
        let store = drafts(&root);
        let (c, _) = hub_with_qt(&store, Some(qt.clone()));
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        let not_restored = drafts::NOT_RESTORED.to_owned();
        let detail = c.saved_draft(DraftSource::Qt, KEY);
        assert_eq!(
            (detail.detail.clone(), detail.restorable),
            (not_restored.clone(), false)
        );
        assert_eq!(
            c.restore(view.handle, DraftSource::Qt, KEY),
            Err(not_restored.clone())
        );
        assert_eq!(c.recover(KEY), Err(not_restored));
        c.flush();
        store.flush();
        assert!(
            !root.join("drafts").exists() || snapshot(&root.join("drafts")).is_empty(),
            "nothing was copied here"
        );
        assert_eq!(snapshot(&qt), qt_before);
    }

    #[test]
    fn a_recovered_draft_outlasts_an_empty_save_queued_while_it_ran() {
        let root = dir("recover-queued");
        let qt = qt_store_with_a_draft(&root);
        let store = drafts(&root);
        let (c, _) = hub_with_qt(&store, Some(qt));
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        // The writer is held: the recover waits, and an edit of the empty
        // draft (a cursor move) queues its save behind it.
        let release = crate::composer::tests::hold(&store);
        let recovering = {
            let c = c.clone();
            std::thread::spawn(move || c.recover(KEY))
        };
        std::thread::sleep(std::time::Duration::from_millis(100));
        c.edit(view.handle, 1, view.text_epoch, String::new(), 0, 0);
        release.send(()).unwrap();
        assert_eq!(recovering.join().unwrap(), Ok(()));
        until(&c, |v| v.text == "from the Qt app");
        c.flush();
        store.flush();
        assert_eq!(
            read_draft(&root.join("drafts"), KEY).draft.draft.text,
            "from the Qt app",
            "the composer that took the draft saved it after the empty save"
        );
    }

    #[test]
    fn restoring_a_qt_draft_keeps_a_copy_here() {
        let root = dir("restore-qt-copy");
        let qt = qt_store_with_a_draft(&root);
        let store = drafts(&root);
        let (c, _) = hub_with_qt(&store, Some(qt));
        c.open(Some(crate::composer::send::tests::other_session()));
        let view = until(&c, |v| v.loaded);
        assert_eq!(c.restore(view.handle, DraftSource::Qt, KEY), Ok(()));
        assert_eq!(c.current().unwrap().text, "from the Qt app");
        c.flush();
        store.flush();
        assert_eq!(
            read_draft(&root.join("drafts"), KEY).draft.draft.text,
            "from the Qt app",
            "the Qt draft is also listed on this computer"
        );
    }
}
