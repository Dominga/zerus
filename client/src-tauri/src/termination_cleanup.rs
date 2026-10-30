//! Explicit post-Terminate cleanup. Existing core state machines and guarded
//! CLI calls own every decision; this module holds bounded opaque UI handles.
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, State};
use zerus_client_core::{default_hgs, run_limited, ComputerId, Exit, Invocation, Outcome, Target};
use zerus_client_core::{
    termination_cleanup::{Capture, Cleanup, Consents, Status, Step, STATE_CHANGED},
    worktree_review as review, worktrees,
};

const LIMIT: usize = review::CLEANUP_TARGETS;
const PREF: &str = "workspace/removeWorktreeAfterTermination";
const OFFER_AGE: Duration = Duration::from_secs(10 * 60);
#[derive(Default)]
pub struct TerminationCleanup(Arc<Inner>);
#[derive(Default)]
struct Inner {
    model: Mutex<Model>,
}
#[derive(Default)]
struct Model {
    offers: VecDeque<Offer>,
    jobs: VecDeque<Job>,
    reserved: usize,
    reads: BTreeSet<String>,
    revision: u64,
}
struct Offer {
    id: String,
    capture: Capture,
    at: Instant,
}
struct Job {
    id: String,
    cleanup: Cleanup,
    busy: bool,
    phase: &'static str,
    manual_review: bool,
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
fn bounded_run(hgs: &Path, inv: &Invocation) -> Exit {
    run_limited(hgs, inv, &[], review::REVIEW_LIMIT).unwrap_or(Exit::StartFailed(
        "The reply is too large. Check the folder before continuing.".into(),
    ))
}
pub(crate) fn probe(
    hgs: &Path,
    target: &Target,
    runner: &impl Fn(&Path, &Invocation) -> Exit,
) -> Result<(), String> {
    let mut inv = zerus_client_core::details::details_invocation(target, None);
    inv = worktrees::guard_probe(&inv);
    inv.timeout = Duration::from_secs(5);
    let exit = runner(hgs, &inv);
    let supported = matches!(&exit, Exit::Code{code:0,stdout,..} if stdout.len() <= 64*1024 && String::from_utf8_lossy(stdout).contains("scoped native lifecycle action"));
    worktrees::guard_supported(exit)?;
    if !supported {
        return Err("Update hgs before removing a working folder after Terminate.".into());
    }
    Ok(())
}
pub(crate) fn inspection(
    hgs: &Path,
    target: &Target,
    machine: &str,
    runner: &impl Fn(&Path, &Invocation) -> Exit,
) -> Result<Value, String> {
    let mut inv = zerus_client_core::details::details_invocation(target, None);
    worktrees::guard(&mut inv.args, machine);
    match runner(hgs, &inv) {
        Exit::Code {
            code: 0, stdout, ..
        } if stdout.len() <= review::REVIEW_LIMIT => {
            serde_json::from_slice(&stdout).map_err(|_| STATE_CHANGED.into())
        }
        _ => Err(STATE_CHANGED.into()),
    }
}

struct ReadGuard(Arc<Inner>, String);
impl Drop for ReadGuard {
    fn drop(&mut self) {
        lock(&self.0.model).reads.remove(&self.1);
    }
}
impl Inner {
    fn read(self: &Arc<Self>) -> Result<ReadGuard, String> {
        let mut m = lock(&self.model);
        if m.reads.len() >= 4 {
            return Err(
                "A folder inspection is already running. Try again after it finishes.".into(),
            );
        }
        let id = uuid::Uuid::new_v4().to_string();
        m.reads.insert(id.clone());
        Ok(ReadGuard(Arc::clone(self), id))
    }
}

#[derive(Serialize)]
pub struct OfferView {
    id: String,
    checked: bool,
}
#[derive(Serialize)]
pub struct Row {
    id: String,
    label: String,
    path: String,
    machine: String,
    computer: String,
    branch: String,
    size: String,
    status: &'static str,
    reason: String,
    details: String,
    can_check: bool,
    ready: bool,
}
#[derive(Serialize)]
pub struct View {
    revision: u64,
    rows: Vec<Row>,
    busy: bool,
    removed: usize,
    kept: usize,
    unknown: usize,
    required: Consents,
    text: String,
    title: String,
}
fn leaf(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .into()
}
fn ready(entry: &zerus_client_core::termination_cleanup::Entry) -> bool {
    !matches!(entry.status, Some(Status::Removed | Status::Unconfirmed))
        && entry.verified
        && (review::removable(&entry.row)
            || review::process_reviewable(&entry.row)
            || (entry.row["discardable"] == true
                && entry.row["discard_fingerprint"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())))
}
impl Model {
    fn view(&self) -> View {
        let mut rows = Vec::new();
        let (mut removed, mut kept, mut unknown) = (0, 0, 0);
        let mut required = Consents::default();
        for job in &self.jobs {
            for entry in &job.cleanup.entries {
                let path = entry.row["path"].as_str().unwrap_or("");
                let status = match entry.status {
                    Some(Status::Removed) => {
                        removed += 1;
                        "removed"
                    }
                    Some(Status::Unconfirmed) => {
                        unknown += 1;
                        "unknown"
                    }
                    Some(Status::Kept) => {
                        kept += 1;
                        "kept"
                    }
                    None => {
                        if job.busy {
                            job.phase
                        } else {
                            "ready"
                        }
                    }
                };
                let reason = if !entry.detail.is_empty() {
                    entry.detail.clone()
                } else if !entry.review_error.is_empty() {
                    entry.review_error.clone()
                } else if entry.status == Some(Status::Kept) {
                    job.cleanup.retained_reason().unwrap_or_default()
                } else {
                    review::summary(&entry.row).join(" ")
                };
                let r = if job.manual_review {
                    job.cleanup.required()
                } else {
                    Consents::default()
                };
                required.local_files |= r.local_files;
                required.discard_changes |= r.discard_changes;
                required.unchecked_processes |= r.unchecked_processes;
                let details=format!("Folder: {path}\nComputer: {}\n{}\n{}\nAfter removal: conversations and branches remain. Restoring an archived session may require recreating its working folder.", entry.machine,reason,review::decision(&entry.row).join("\n"));
                rows.push(Row {
                    id: job.id.clone(),
                    label: leaf(path),
                    path: path.into(),
                    machine: if entry.host.is_empty() {
                        "this computer".into()
                    } else {
                        entry.host.clone()
                    },
                    computer: entry.host.clone(),
                    branch: entry.row["branch"].as_str().unwrap_or("—").into(),
                    size: review::size(&entry.row),
                    status,
                    reason: reason.chars().take(4000).collect(),
                    details: details.chars().take(128 * 1024).collect(),
                    can_check: !job.busy && job.cleanup.can_refresh(),
                    ready: !job.busy && job.manual_review && ready(entry),
                });
            }
        }
        let busy = self.jobs.iter().any(|j| j.busy);
        let text = if busy {
            "Cleaning working folders…".into()
        } else if kept == 1 && unknown == 0 && removed == 0 {
            format!(
                "Folder kept: {}.",
                rows.first().map(|r| r.label.as_str()).unwrap_or("")
            )
        } else {
            // Only what the chip still holds: a finished removal has its own
            // notice, so a "0 removed" beside it would read as a contradiction.
            let parts: Vec<String> = [(removed, "removed"), (kept, "kept"), (unknown, "unknown")]
                .into_iter()
                .filter(|(count, _)| *count > 0)
                .map(|(count, word)| format!("{count} {word}"))
                .collect();
            if parts.is_empty() {
                "Working folders need review".into()
            } else {
                format!("Working folders: {}", parts.join("  "))
            }
        };
        let mut title = text.clone();
        if let Some(reason) = self.jobs.iter().find_map(|j| j.cleanup.retained_reason()) {
            title.push(' ');
            title.push_str(&reason);
        }
        if unknown > 0 {
            title.push_str(" An unknown result will not be retried.");
        }
        View {
            revision: self.revision,
            rows,
            busy,
            removed,
            kept,
            unknown,
            required,
            text,
            title,
        }
    }
}
fn changed(app: &AppHandle, model: &mut Model) {
    model.revision = model.revision.wrapping_add(1);
    let _ = app.emit("termination-cleanup-changed", ());
}
impl Inner {
    fn drive(self: &Arc<Self>, app: AppHandle, id: String, mut step: Step) {
        let own = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            loop {
                match step {
                    Step::Review {
                        host,
                        common_dir,
                        worktree,
                        ..
                    } => {
                        let mut inv = review::review_invocation(
                            &ComputerId(host.clone()),
                            &common_dir,
                            &worktree,
                        );
                        // Every follow-up stays bound to the original owning computer.
                        let machine = {
                            let m = lock(&own.model);
                            let Some(j) = m.jobs.iter().find(|j| j.id == id) else {
                                return;
                            };
                            j.cleanup.entries[0].machine.clone()
                        };
                        worktrees::guard(&mut inv.args, &machine);
                        let result = worktrees::run_guarded(&default_hgs(), &inv, bounded_run)
                            .and_then(|exit| review::review(exit, &host, false))
                            .and_then(|value| {
                                if serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() <= 1024 * 1024) {
                                    Ok(value)
                                } else {
                                    Err("The folder review is too large. Inspect it in Projects > Worktrees.".into())
                                }
                            });
                        let mut m = lock(&own.model);
                        let Some(j) = m.jobs.iter_mut().find(|j| j.id == id) else {
                            return;
                        };
                        step = j.cleanup.reviewed(
                            &host,
                            &common_dir,
                            result.as_ref().map_err(Clone::clone),
                        );
                        j.phase = if matches!(step, Step::Remove { .. }) {
                            "removing"
                        } else {
                            "checking"
                        };
                        changed(&app, &mut m);
                    }
                    Step::Remove { host, removal, .. } => {
                        let request = uuid::Uuid::new_v4().to_string();
                        let mut inv = review::remove_invocation(
                            &ComputerId(host.clone()),
                            &removal,
                            &request,
                        );
                        let machine = {
                            let m = lock(&own.model);
                            let Some(j) = m.jobs.iter().find(|j| j.id == id) else {
                                return;
                            };
                            j.cleanup.entries[0].machine.clone()
                        };
                        worktrees::guard(&mut inv.args, &machine);
                        let result = match worktrees::run_guarded(&default_hgs(), &inv, bounded_run)
                        {
                            Ok(exit) => review::removed(exit, &host, &request, &removal),
                            Err(detail) => review::Removed::Refused {
                                reply: json!({"status":"kept"}),
                                detail,
                            },
                        };
                        let mut m = lock(&own.model);
                        let Some(j) = m.jobs.iter_mut().find(|j| j.id == id) else {
                            return;
                        };
                        step = j.cleanup.removed(&result);
                        changed(&app, &mut m);
                    }
                    Step::Idle | Step::Done => {
                        let mut m = lock(&own.model);
                        let Some(j) = m.jobs.iter_mut().find(|j| j.id == id) else {
                            return;
                        };
                        j.busy = false;
                        if j.cleanup
                            .entries
                            .iter()
                            .all(|e| e.status == Some(Status::Removed))
                        {
                            let labels = j
                                .cleanup
                                .entries
                                .iter()
                                .map(|e| leaf(e.row["path"].as_str().unwrap_or("")))
                                .collect::<Vec<_>>();
                            m.jobs.retain(|j| j.id != id);
                            for label in labels {
                                let _ = app.emit("termination-folder-removed", label);
                            }
                        }
                        changed(&app, &mut m);
                        break;
                    }
                }
            }
        });
    }
}
#[tauri::command]
pub fn termination_cleanup_view(state: State<'_, TerminationCleanup>) -> View {
    lock(&state.0.model).view()
}
#[tauri::command]
pub async fn termination_cleanup_offer(
    app: State<'_, crate::commands::AppState>,
    state: State<'_, TerminationCleanup>,
    settings: State<'_, crate::settings::Store>,
    target: Target,
) -> Result<Option<OfferView>, String> {
    if crate::actions::blocked() {
        return Ok(None);
    }
    let row = app
        .pump()
        .row_named(&target.computer, &target.name)
        .ok_or(STATE_CHANGED)?;
    if row.state != "running"
        || row.run_id.as_deref() != Some(&target.run_id)
        || row.conversation_id.as_deref() != Some(&target.conversation_id)
        || row.machine.is_empty()
    {
        return Err(STATE_CHANGED.into());
    }
    let _read = state.0.read()?;
    let call_target = target.clone();
    let machine = app
        .pump()
        .machine(&ComputerId(target.computer.clone()))
        .and_then(|m| m.machine_id)
        .filter(|id| zerus_client_core::creation::canonical_uuid(id))
        .ok_or(STATE_CHANGED)?;
    let capture = tauri::async_runtime::spawn_blocking(move || {
        let hgs = default_hgs();
        probe(&hgs, &call_target, &bounded_run)?;
        let value = inspection(&hgs, &call_target, &machine, &bounded_run)?;
        Capture::from_inspection(&call_target, &machine, &value)
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some(capture) = capture else {
        return Ok(None);
    };
    let mut m = lock(&state.0.model);
    m.offers.retain(|o| o.at.elapsed() < OFFER_AGE);
    if m.offers.len() + m.jobs.len() + m.reserved >= LIMIT {
        return Err("Review the retained working folders before starting another cleanup.".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    m.offers.push_back(Offer {
        id: id.clone(),
        capture,
        at: Instant::now(),
    });
    Ok(Some(OfferView {
        id,
        checked: settings.0.get(PREF).as_bool().unwrap_or(false),
    }))
}
#[tauri::command]
pub fn termination_cleanup_cancel(state: State<'_, TerminationCleanup>, id: String) {
    lock(&state.0.model).offers.retain(|o| o.id != id);
}
#[tauri::command]
pub async fn termination_cleanup_confirm(
    app: AppHandle,
    state: State<'_, TerminationCleanup>,
    actions: State<'_, crate::actions::Actions>,
    settings: State<'_, crate::settings::Store>,
    id: String,
) -> Result<Outcome, String> {
    if crate::actions::blocked() {
        return Err(crate::actions::REPLAY_NEEDS_HGS.into());
    }
    let capture = {
        let mut m = lock(&state.0.model);
        let index = m
            .offers
            .iter()
            .position(|o| o.id == id)
            .ok_or(STATE_CHANGED)?;
        let o = m.offers.remove(index).ok_or(STATE_CHANGED)?;
        if o.at.elapsed() >= OFFER_AGE {
            return Err(STATE_CHANGED.into());
        }
        m.reserved += 1;
        o.capture
    };
    // A remembered checkbox default never changes the action's admission.
    let _ = settings.0.set(PREF, Value::Bool(true));
    let outcome = crate::actions::dispatch_cleanup(&actions, capture.clone()).await;
    let mut m = lock(&state.0.model);
    m.reserved -= 1;
    if let Ok(Outcome::Done { cleanup, .. }) = &outcome {
        if let Some(entry) = capture.entry(cleanup.as_ref()) {
            let mut cleanup = Cleanup::after_termination(vec![entry]);
            let step = cleanup.step();
            m.jobs.push_back(Job {
                id: id.clone(),
                cleanup,
                busy: true,
                phase: "checking",
                manual_review: false,
            });
            changed(&app, &mut m);
            drop(m);
            state.0.drive(app, id, step);
        }
    }
    outcome
}
#[tauri::command]
pub fn termination_cleanup_check(
    app: AppHandle,
    state: State<'_, TerminationCleanup>,
    id: String,
) -> Result<View, String> {
    let mut m = lock(&state.0.model);
    let j = m
        .jobs
        .iter_mut()
        .find(|j| j.id == id)
        .ok_or(STATE_CHANGED)?;
    if j.busy || !j.cleanup.can_refresh() {
        return Err("An unknown result will not be retried.".into());
    }
    j.manual_review = true;
    j.cleanup.review_remaining();
    let step = j.cleanup.step();
    j.busy = true;
    j.phase = "checking";
    changed(&app, &mut m);
    let view = m.view();
    drop(m);
    state.0.drive(app, id, step);
    Ok(view)
}
#[tauri::command]
pub fn termination_cleanup_remove(
    app: AppHandle,
    state: State<'_, TerminationCleanup>,
    revision: u64,
    mut consents: Consents,
) -> Result<View, String> {
    let mut m = lock(&state.0.model);
    if m.revision != revision || m.jobs.iter().any(|j| j.busy) {
        return Err("Folder changed. Refresh its cleanup review.".into());
    }
    consents.merged_branches = false;
    let mut calls = Vec::new();
    for j in &mut m.jobs {
        if !j.manual_review {
            continue;
        }
        let step = j.cleanup.remove(consents);
        if matches!(step, Step::Remove { .. }) {
            j.busy = true;
            j.phase = "removing";
            j.manual_review = false;
            calls.push((j.id.clone(), step));
        }
    }
    changed(&app, &mut m);
    let view = m.view();
    drop(m);
    for (id, step) in calls {
        state.0.drive(app.clone(), id, step)
    }
    Ok(view)
}
#[tauri::command]
pub fn termination_cleanup_keep(
    app: AppHandle,
    state: State<'_, TerminationCleanup>,
    revision: u64,
) -> Result<View, String> {
    let mut m = lock(&state.0.model);
    if m.revision != revision || m.jobs.iter().any(|j| j.busy) {
        return Err("Cleanup changed or is still running. Review the recorded outcomes.".into());
    }
    m.jobs.clear();
    changed(&app, &mut m);
    Ok(m.view())
}

/// Read-only inspection of an opaque recorded row; never accepts a local root
/// from the webview and never refreshes its removal fingerprint implicitly.
#[tauri::command]
pub async fn termination_cleanup_inspect(
    state: State<'_, TerminationCleanup>,
    id: String,
    file: String,
    offset: usize,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::actions::REPLAY_NEEDS_HGS.into());
    }
    if file.len() > 4096 || file.chars().any(char::is_control) || offset > 1_000_000 {
        return Err(STATE_CHANGED.into());
    }
    let _read = state.0.read()?;
    let (host, machine, path, common) = {
        let m = lock(&state.0.model);
        let j = m.jobs.iter().find(|j| j.id == id).ok_or(STATE_CHANGED)?;
        if j.busy {
            return Err("Cleanup is still running.".into());
        }
        let e = &j.cleanup.entries[0];
        (
            e.host.clone(),
            e.machine.clone(),
            e.row["path"].as_str().unwrap_or("").to_owned(),
            e.common_dir.clone(),
        )
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mut inv = review::inspect_invocation(&ComputerId(host), &path, &common, &file, offset);
        worktrees::guard(&mut inv.args, &machine);
        let exit = worktrees::run_guarded(&default_hgs(), &inv, bounded_run)?;
        review::inspected(exit, &path, &common, &file, offset)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_client_core::termination_cleanup::Entry;
    fn job(status: Option<Status>) -> Job {
        let mut entry = Entry::terminated(
            "example-peer",
            "machine-1",
            "/srv/example/work",
            "/srv/example/.git",
            "owner-1",
            None,
        )
        .unwrap();
        entry.status = status;
        Job {
            id: "job-1".into(),
            cleanup: Cleanup::after_termination(vec![entry]),
            busy: false,
            phase: "checking",
            manual_review: false,
        }
    }
    #[test]
    fn retained_status_is_passive_and_unknown_never_becomes_a_ready_target() {
        let mut m = Model::default();
        m.jobs.push_back(job(Some(Status::Unconfirmed)));
        let view = m.view();
        assert_eq!(view.unknown, 1);
        assert!(!view.rows[0].can_check);
        assert!(!view.rows[0].ready);
        assert!(view
            .title
            .contains("An unknown result will not be retried."));
        let original = m.jobs[0].cleanup.clone();
        let _ = m.view();
        assert_eq!(
            m.jobs[0].cleanup, original,
            "opening only reads recorded outcomes"
        );
        m.jobs.push_back(job(Some(Status::Kept)));
        let view = m.view();
        assert_eq!(view.kept, 1);
        assert!(view.rows[1].can_check);
        assert_eq!(view.text, "Working folders: 1 kept  1 unknown");
        m.jobs[1].busy = true;
        assert_eq!(m.view().text, "Cleaning working folders…");
    }
    #[test]
    fn a_background_kept_folder_is_not_a_manual_removal_without_check_again() {
        let mut j = job(Some(Status::Kept));
        j.cleanup.entries[0].verified = true;
        j.cleanup.entries[0].row =
            json!({"path":"/srv/example/work","removable":true,"fingerprint":"f"});
        let mut m = Model::default();
        m.jobs.push_back(j);
        assert!(!m.view().rows[0].ready);
        m.jobs[0].manual_review = true;
        // The UI-ready predicate shares the existing core validator.
        assert_eq!(m.view().rows[0].ready, ready(&m.jobs[0].cleanup.entries[0]));
    }
    #[test]
    fn current_inspection_reads_are_bounded_and_charge_until_their_guard_ends() {
        let inner = Arc::new(Inner::default());
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(inner.read().unwrap())
        }
        assert!(inner.read().is_err());
        held.pop();
        assert!(inner.read().is_ok());
        drop(held);
        assert!(lock(&inner.model).reads.is_empty());
    }
    #[test]
    fn old_or_oversized_help_never_admits_the_optional_scoped_action() {
        let target = Target {
            computer: "example-peer".into(),
            name: "codex/example/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        };
        for text in ["guarded-create-v1", "scoped native lifecycle action", ""] {
            assert!(probe(Path::new("fake-hgs"), &target, &|_, inv| {
                assert_eq!(inv.args, ["@example-peer", "--help"]);
                assert_eq!(inv.timeout.as_secs(), 5);
                Exit::Code {
                    code: 0,
                    stdout: text.as_bytes().to_vec(),
                    stderr: String::new(),
                }
            })
            .is_err());
        }
        assert!(probe(Path::new("fake-hgs"), &target, &|_, _| Exit::Code {
            code: 0,
            stdout: b"guarded-create-v1 scoped native lifecycle action".to_vec(),
            stderr: String::new()
        })
        .is_ok());
    }
}
