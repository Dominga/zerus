//! The explicit review of the desktop service's held task message input
//! copies (zerus-pi2k9). A page is one bounded read-only listing that only a
//! person's click requests. A release names exactly what this client's latest
//! listing of that record observed, and each observation is sent at most
//! once. The service rechecks everything it is sent; this client neither
//! retries a release nor guesses its outcome.
use super::creation_clone::Read;
use super::*;
use serde::{Deserialize, Serialize};
use zerus_client_core::desktop::{
    task_message::{
        review::{self, Cursor, Entry, List, Observation, Outcome, Page, Release, Released},
        Scope,
    },
    ui_requests::Request,
};

/// The most reviewed records remembered: the service charges at most 64.
const REVIEWED: usize = 64;

/// One remembered review: the listing that observed it, the scope that
/// listed it and the entry.
struct Review {
    listing: u64,
    scope: Scope,
    entry: Entry,
}

/// The newest listing's entry of each listed record. Listings are numbered
/// when they start, so a read that ends after a newer one began (a dialog
/// closed mid-read, then reopened) never replaces it, and a confirmation
/// names the listing whose row it confirms.
#[derive(Default)]
pub(super) struct Reviewed {
    listings: std::sync::atomic::AtomicU64,
    reviews: Mutex<BTreeMap<String, Review>>,
}
impl Reviewed {
    /// Numbers one listing as it starts.
    fn begin(&self) -> u64 {
        self.listings.fetch_add(1, Ordering::AcqRel) + 1
    }
    fn remember(&self, listing: u64, scope: &Scope, entries: &[Entry]) {
        let mut map = self.reviews.lock().unwrap_or_else(PoisonError::into_inner);
        for entry in entries {
            if map
                .get(&entry.request_id)
                .is_some_and(|known| known.listing > listing)
            {
                continue;
            }
            map.insert(
                entry.request_id.clone(),
                Review {
                    listing,
                    scope: scope.clone(),
                    entry: entry.clone(),
                },
            );
        }
        while map.len() > REVIEWED {
            let oldest = map
                .iter()
                .min_by_key(|(_, review)| review.listing)
                .map(|(id, _)| id.clone())
                .expect("a non-empty map");
            map.remove(&oldest);
        }
    }
    /// Takes the entry `listing` observed: one confirmation sends it once,
    /// and a confirmation of any other listing sends nothing.
    fn take(&self, request_id: &str, listing: u64) -> Option<(Scope, Entry)> {
        let mut map = self.reviews.lock().unwrap_or_else(PoisonError::into_inner);
        if !map
            .get(request_id)
            .is_some_and(|review| review.listing == listing)
        {
            return None;
        }
        map.remove(request_id)
            .map(|review| (review.scope, review.entry))
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

/// One page, the listing that observed it, and where the next continues.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PageView {
    pub listing: u64,
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
    let listing = read.service.shared.held_inputs.begin();
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
        .remember(listing, &page.scope, &page.entries);
    Ok(PageView {
        listing,
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

/// The one release of exactly what `listing` observed of `request_id`.
pub(crate) fn release(service: Option<Arc<Service>>, request_id: &str, listing: u64) -> Answer {
    let Some((scope, entry)) = service
        .as_ref()
        .and_then(|service| service.shared.held_inputs.take(request_id, listing))
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
        .ok();
    answer(reply.as_deref(), &release)
}

/// The review's own refusal before any claim.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewRefusal {
    abi: String,
    read_only: bool,
    #[allow(dead_code)]
    code: String,
}
/// The native lane's refusal before the review saw the request. The service
/// always writes `request_id` (null for a release); a missing key is not
/// this shape.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaneRefusal {
    #[serde(deserialize_with = "Option::deserialize")]
    request_id: Option<String>,
    accepted: bool,
    durable: bool,
    #[allow(dead_code)]
    code: String,
}

/// What `reply` says about `release`. Only this release's own reply or an
/// exact refusal before any claim is definite; anything else, a lost reply
/// included, is Unknown.
fn answer(reply: Option<&[u8]>, release: &Release) -> Answer {
    let Some(bytes) = reply else {
        return Answer::Unknown;
    };
    if let Ok(released) = serde_json::from_slice::<Released>(bytes) {
        if !released.matches(release) {
            return Answer::Unknown;
        }
        return match (released.outcome, released.code.as_str()) {
            (Outcome::Removed, _) => Answer::Removed,
            (Outcome::Released, _) => Answer::Released,
            (Outcome::Kept, review::CHANGED) => Answer::Changed,
            (Outcome::Kept, _) => Answer::Kept,
            (Outcome::Unknown, _) => Answer::Unknown,
        };
    }
    let refused = serde_json::from_slice::<ReviewRefusal>(bytes)
        .is_ok_and(|r| r.abi == zerus_client_core::desktop::task_message::ABI && !r.read_only)
        || serde_json::from_slice::<LaneRefusal>(bytes).is_ok_and(|r| {
            !r.accepted
                && !r.durable
                && r.request_id
                    .as_deref()
                    .is_none_or(|id| id == release.release_id)
        });
    if refused {
        Answer::Kept
    } else {
        Answer::Unknown
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
    listing: u64,
) -> Result<Answer, String> {
    let service = source.desktop_service().filter(|_| mode());
    Ok(
        tauri::async_runtime::spawn_blocking(move || release(service, &request_id, listing))
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
    fn a_reviewed_entry_is_taken_once_and_only_by_its_own_listing() {
        let reviewed = Reviewed::default();
        let (first, second) = (reviewed.begin(), reviewed.begin());
        assert!(first < second);
        reviewed.remember(first, &scope(1), &[entry(1, Observation::Absent)]);
        let fresh = entry(1, Observation::Present { version: version() });
        reviewed.remember(second, &scope(2), std::slice::from_ref(&fresh));
        let id = fresh.request_id.clone();
        assert_eq!(
            reviewed.take(&id, first),
            None,
            "an older row sends nothing"
        );
        assert_eq!(reviewed.take(&id, second), Some((scope(2), fresh)));
        assert_eq!(reviewed.take(&id, second), None, "sent at most once");
    }

    #[test]
    fn a_read_that_ends_late_never_replaces_a_newer_listing() {
        let reviewed = Reviewed::default();
        // Closed mid-read, then reopened: the reopened dialog's read starts
        // later and ends first.
        let (stale, newer) = (reviewed.begin(), reviewed.begin());
        let fresh = entry(1, Observation::Present { version: version() });
        reviewed.remember(newer, &scope(1), std::slice::from_ref(&fresh));
        reviewed.remember(stale, &scope(1), &[entry(1, Observation::Absent)]);
        let id = fresh.request_id.clone();
        assert_eq!(reviewed.take(&id, stale), None);
        assert_eq!(reviewed.take(&id, newer), Some((scope(1), fresh)));
    }

    #[test]
    fn the_remembered_reviews_stay_bounded_and_evict_the_oldest() {
        let reviewed = Reviewed::default();
        let entries: Vec<Entry> = (0..=REVIEWED as u8)
            .map(|n| entry(n, Observation::Absent))
            .collect();
        let old = reviewed.begin();
        reviewed.remember(old, &scope(1), &entries[..1]);
        let new = reviewed.begin();
        reviewed.remember(new, &scope(1), &entries[1..]);
        assert!(reviewed.take(&entries[0].request_id, old).is_none());
        for entry in &entries[1..] {
            assert!(reviewed.take(&entry.request_id, new).is_some());
        }
    }

    #[test]
    fn only_this_release_or_an_exact_refusal_is_definite() {
        let release = Release {
            scope: scope(1),
            release_id: "00000000-0000-4000-8000-000000000003".into(),
            request_id: "00000000-0000-4000-8000-000000000002".into(),
            state: "orphaned".into(),
            bytes: 10,
            input_sha256: "a".repeat(64),
            observed: None,
        };
        let reply = |outcome, code: &str| {
            serde_json::to_vec(&Released {
                abi: zerus_client_core::desktop::task_message::ABI.into(),
                release_id: release.release_id.clone(),
                request_id: release.request_id.clone(),
                scope: scope(1),
                outcome,
                code: code.into(),
            })
            .unwrap()
        };
        let read = |bytes: &[u8]| answer(Some(bytes), &release);
        assert_eq!(read(&reply(Outcome::Removed, "")), Answer::Removed);
        assert_eq!(read(&reply(Outcome::Released, "")), Answer::Released);
        assert_eq!(
            read(&reply(Outcome::Kept, review::CHANGED)),
            Answer::Changed
        );
        assert_eq!(
            read(&reply(Outcome::Kept, "task_message_input_review_busy")),
            Answer::Kept
        );
        assert_eq!(
            read(&reply(Outcome::Unknown, review::CHANGED)),
            Answer::Unknown
        );
        assert_eq!(answer(None, &release), Answer::Unknown);
        // Another release's reply is never this one's.
        let mut other: Value = serde_json::from_slice(&reply(Outcome::Removed, "")).unwrap();
        other["release_id"] = json!("00000000-0000-4000-8000-000000000009");
        assert_eq!(read(&serde_json::to_vec(&other).unwrap()), Answer::Unknown);
        // Exact refusals before any claim: nothing changed.
        for refusal in [
            json!({"abi": zerus_client_core::desktop::task_message::ABI, "read_only": false,
                "code": "task_message_input_review_refused"}),
            json!({"request_id": null, "accepted": false, "durable": false,
                "code": "frontend_authentication_required"}),
            json!({"request_id": release.release_id, "accepted": false, "durable": false,
                "code": "task_message_input_release_unsupported"}),
        ] {
            assert_eq!(
                read(&serde_json::to_vec(&refusal).unwrap()),
                Answer::Kept,
                "{refusal}"
            );
        }
        // Anything else stays unknown.
        for unclear in [
            json!({"abi": "desktop-task-message-v0", "read_only": false, "code": "x"}),
            json!({"abi": zerus_client_core::desktop::task_message::ABI, "read_only": true, "code": "x"}),
            json!({"request_id": "00000000-0000-4000-8000-000000000009", "accepted": false,
                "durable": false, "code": "x"}),
            json!({"request_id": null, "accepted": true, "durable": false, "code": "x"}),
            json!({"request_id": null, "accepted": false, "durable": false, "code": "x", "extra": 1}),
            json!({"request_id": null, "accepted": false, "durable": true, "code": "x"}),
            json!({"accepted": false, "durable": false, "code": "x"}),
            json!({"abi": zerus_client_core::desktop::task_message::ABI, "read_only": false,
                "code": "x", "release_id": release.release_id}),
            json!({}),
        ] {
            assert_eq!(
                read(&serde_json::to_vec(&unclear).unwrap()),
                Answer::Unknown,
                "{unclear}"
            );
        }
        assert_eq!(read(b"not json"), Answer::Unknown);
    }

    #[test]
    fn nothing_is_sent_without_a_reviewed_observation_or_a_service() {
        assert_eq!(release(None, "x", 1), Answer::NotSent);
    }
}
