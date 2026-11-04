//! The explicit review of the desktop service's held task message input
//! copies (zerus-pi2k9). A page is one bounded read-only listing that only a
//! person's click requests. A release names exactly what this client's latest
//! listing of that record observed, and each observation is sent at most
//! once. The service rechecks everything it is sent; this client neither
//! retries a release nor guesses its outcome.
use super::creation_clone::Read;
use super::*;
use serde::Serialize;
use zerus_client_core::desktop::{
    task_message::{
        review::{self, Cursor, Entry, List, Observation, Outcome, Page, Release, Released},
        Scope,
    },
    ui_requests::Request,
};

/// The most reviewed records remembered: the service charges at most 64.
const REVIEWED: usize = 64;

/// Each remembered review: its listing order, the scope that listed it and
/// the entry, by request ID.
type Reviews = BTreeMap<String, (u64, Scope, Entry)>;

/// The latest reviewed entry of each listed record, with the scope that
/// listed it, in listing order for eviction.
#[derive(Default)]
pub(super) struct Reviewed(Mutex<(u64, Reviews)>);
impl Reviewed {
    fn remember(&self, scope: &Scope, entries: &[Entry]) {
        let mut guard = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (order, map) = &mut *guard;
        for entry in entries {
            *order += 1;
            map.insert(
                entry.request_id.clone(),
                (*order, scope.clone(), entry.clone()),
            );
        }
        while map.len() > REVIEWED {
            let oldest = map
                .iter()
                .min_by_key(|(_, (order, _, _))| *order)
                .map(|(id, _)| id.clone())
                .expect("a non-empty map");
            map.remove(&oldest);
        }
    }
    /// Takes the reviewed entry: one confirmation sends it once.
    fn take(&self, request_id: &str) -> Option<(Scope, Entry)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
            .remove(request_id)
            .map(|(_, scope, entry)| (scope, entry))
    }
}

/// What the renderer shows of one record: no hash, no version.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Row {
    pub request_id: String,
    /// Milliseconds since the epoch.
    pub created: i64,
    pub bytes: u64,
    /// `orphaned` (not sent) or `failed` (sent; copy not removed).
    pub state: String,
    /// `present`, `missing`, or `unsafe` (never released from this review).
    pub file: &'static str,
}

/// One page, and where the next continues.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PageView {
    pub rows: Vec<Row>,
    pub next: Option<Cursor>,
}

fn row(entry: &Entry) -> Row {
    Row {
        request_id: entry.request_id.clone(),
        created: entry.created,
        bytes: entry.bytes,
        state: entry.state.clone(),
        file: match entry.observed {
            Observation::Present { .. } => "present",
            Observation::Absent => "missing",
            Observation::Unsafe => "unsafe",
        },
    }
}

fn read(service: Option<Arc<Service>>) -> Result<Arc<Read>, String> {
    let read = Read::new(service)?;
    if !read.supports(review::HELP_MARKER)? {
        return Err("task_message_inputs_unsupported".into());
    }
    Ok(read)
}

/// One bounded read-only page after `after`.
pub(crate) fn list(
    service: Option<Arc<Service>>,
    after: Option<Cursor>,
) -> Result<PageView, String> {
    let read = read(service)?;
    let list = List {
        op: review::LIST_OP.into(),
        scope: read.scope.clone(),
        after,
    };
    if !list.valid() {
        return Err("task_message_inputs_unavailable".into());
    }
    let bytes = read.exchange(None, json!(list), false, None)?;
    let page: Page =
        serde_json::from_slice(&bytes).map_err(|_| "task_message_inputs_unavailable")?;
    if !page.matches(&list) {
        return Err("task_message_inputs_unavailable".into());
    }
    if !page.code.is_empty() {
        return Err(page.code);
    }
    read.service
        .shared
        .held_inputs
        .remember(&page.scope, &page.entries);
    Ok(PageView {
        rows: page.entries.iter().map(row).collect(),
        next: page.next,
    })
}

/// What one confirmation did, as the renderer shows it.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Removed,
    Released,
    /// Nothing changed because the slot, the object or the folder is not
    /// what the review saw: a fresh review decides.
    Changed,
    /// Nothing changed for another reason (busy, a bound, a copy elsewhere,
    /// a stop): a fresh review decides.
    Kept,
    /// Sent, but its effect could not be observed. Never retried.
    Unknown,
    /// Nothing reached the service: no reviewed observation was available
    /// here, or the service changed since the review.
    NotSent,
}

/// The one release of exactly the reviewed observation of `request_id`.
pub(crate) fn release(service: Option<Arc<Service>>, request_id: &str) -> Answer {
    let Some((scope, entry)) = service
        .as_ref()
        .and_then(|service| service.shared.held_inputs.take(request_id))
    else {
        return Answer::NotSent;
    };
    let observed = match entry.observed {
        Observation::Present { version } => Some(version),
        Observation::Absent => None,
        Observation::Unsafe => return Answer::NotSent,
    };
    let Ok(read) = read(service) else {
        return Answer::NotSent;
    };
    if read.scope != scope {
        return Answer::NotSent;
    }
    let release = Release {
        scope,
        release_id: uuid::Uuid::new_v4().to_string(),
        request_id: entry.request_id,
        state: entry.state,
        bytes: entry.bytes,
        input_sha256: entry.input_sha256,
        observed,
    };
    if !release.valid() {
        return Answer::NotSent;
    }
    // From here the request may have been written: any failure is Unknown.
    let reply = read
        .exchange(
            Some(Request::TaskMessageInputRelease {
                release: Box::new(release.clone()),
            }),
            Value::Null,
            false,
            None,
        )
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Released>(&bytes).ok())
        .filter(|released| released.matches(&release));
    answer(reply.as_ref())
}

fn answer(reply: Option<&Released>) -> Answer {
    match reply.map(|released| (released.outcome, released.code.as_str())) {
        Some((Outcome::Removed, _)) => Answer::Removed,
        Some((Outcome::Released, _)) => Answer::Released,
        Some((Outcome::Kept, review::CHANGED)) => Answer::Changed,
        Some((Outcome::Kept, _)) => Answer::Kept,
        Some((Outcome::Unknown, _)) | None => Answer::Unknown,
    }
}

/// One page of held copies, on an explicit click only.
#[tauri::command]
pub async fn desktop_service_task_message_inputs(
    source: tauri::State<'_, crate::commands::AppState>,
    after: Option<Cursor>,
) -> Result<PageView, String> {
    let service = source.desktop_service().filter(|_| mode());
    tauri::async_runtime::spawn_blocking(move || list(service, after))
        .await
        .map_err(|_| "task_message_inputs_unavailable".to_string())?
}

/// One confirmed release of a reviewed copy.
#[tauri::command(rename_all = "snake_case")]
pub async fn desktop_service_task_message_input_release(
    source: tauri::State<'_, crate::commands::AppState>,
    request_id: String,
) -> Result<Answer, String> {
    let service = source.desktop_service().filter(|_| mode());
    Ok(
        tauri::async_runtime::spawn_blocking(move || release(service, &request_id))
            .await
            // The worker may have written the request before it stopped.
            .unwrap_or(Answer::Unknown),
    )
}

#[cfg(all(test, unix))]
mod fixture_tests;
#[cfg(test)]
mod tests {
    use super::*;
    use zerus_client_core::desktop::task_message::review::Version;

    fn scope(epoch: u8) -> Scope {
        Scope {
            configuration: "a".repeat(64),
            service_epoch: format!("00000000-0000-4000-8000-0000000000{epoch:02}"),
        }
    }
    fn entry(n: u8, observed: Observation) -> Entry {
        Entry {
            request_id: format!("00000000-0000-4000-8000-0000000001{n:02}"),
            state: "orphaned".into(),
            bytes: 10,
            input_sha256: "a".repeat(64),
            created: i64::from(n),
            recorded: None,
            observed,
        }
    }
    fn version() -> Version {
        Version {
            device: 1,
            inode: 2,
            size: 10,
            mode: 0o100600,
            links: 1,
            modified: (1, 0),
            changed: (1, 0),
        }
    }

    #[test]
    fn rows_carry_only_display_fields_and_name_the_observed_file() {
        let present = row(&entry(1, Observation::Present { version: version() }));
        assert_eq!(present.file, "present");
        assert_eq!(row(&entry(2, Observation::Absent)).file, "missing");
        assert_eq!(row(&entry(3, Observation::Unsafe)).file, "unsafe");
        let value = serde_json::to_value(present).unwrap();
        assert_eq!(
            value.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["bytes", "created", "file", "request_id", "state"]
        );
    }

    #[test]
    fn a_reviewed_entry_is_taken_once_and_the_newest_review_wins() {
        let reviewed = Reviewed::default();
        reviewed.remember(&scope(1), &[entry(1, Observation::Absent)]);
        let fresh = entry(1, Observation::Present { version: version() });
        reviewed.remember(&scope(2), std::slice::from_ref(&fresh));
        let id = fresh.request_id.clone();
        assert_eq!(reviewed.take(&id), Some((scope(2), fresh)));
        assert_eq!(reviewed.take(&id), None, "sent at most once");
    }

    #[test]
    fn the_remembered_reviews_stay_bounded_and_evict_the_oldest() {
        let reviewed = Reviewed::default();
        let entries: Vec<Entry> = (0..=REVIEWED as u8)
            .map(|n| entry(n, Observation::Absent))
            .collect();
        reviewed.remember(&scope(1), &entries);
        assert!(reviewed.take(&entries[0].request_id).is_none());
        for entry in &entries[1..] {
            assert!(reviewed.take(&entry.request_id).is_some());
        }
    }

    #[test]
    fn only_a_changed_code_reads_as_changed_and_no_reply_is_unknown() {
        let reply = |outcome, code: &str| Released {
            abi: "x".into(),
            release_id: "r".into(),
            request_id: "q".into(),
            scope: scope(1),
            outcome,
            code: code.into(),
        };
        assert_eq!(answer(Some(&reply(Outcome::Removed, ""))), Answer::Removed);
        assert_eq!(
            answer(Some(&reply(Outcome::Released, ""))),
            Answer::Released
        );
        assert_eq!(
            answer(Some(&reply(Outcome::Kept, review::CHANGED))),
            Answer::Changed
        );
        assert_eq!(
            answer(Some(&reply(
                Outcome::Kept,
                "task_message_input_review_busy"
            ))),
            Answer::Kept
        );
        assert_eq!(
            answer(Some(&reply(Outcome::Unknown, review::CHANGED))),
            Answer::Unknown
        );
        assert_eq!(answer(None), Answer::Unknown);
    }

    #[test]
    fn nothing_is_sent_without_a_reviewed_observation_or_a_service() {
        assert_eq!(release(None, "x"), Answer::NotSent);
    }
}
