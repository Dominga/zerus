//! Explicit manual removal of a retained, unowned linked worktree. The opaque
//! source row selects the path; the existing Root manual guards own deletion.
use super::*;

pub(super) struct Offer {
    confirmation: String,
    job: String,
    original: Entry,
    cleanup: Cleanup,
    at: Instant,
}
#[derive(Serialize)]
pub struct ManualView {
    confirmation: String,
    path: String,
    computer: String,
    branch: String,
    size: String,
    changes: u64,
    untracked: u64,
    ignored: Vec<String>,
    ignored_count: u64,
    ignored_size: String,
    required: Consents,
    ready: bool,
    reason: String,
    sessions: Vec<String>,
    processes: Vec<ProgramView>,
}
impl Model {
    pub(super) fn manual_entry(&self, id: &str) -> Result<Entry, String> {
        if self.overflow_unknown > 0 {
            return Err("Too many unconfirmed removals are retained. Review them before removing more worktrees.".into());
        }
        let job = self.jobs.iter().find(|j| j.id == id).ok_or(STATE_CHANGED)?;
        let entry = job.cleanup.entries.first().ok_or(STATE_CHANGED)?;
        let key = presentation::job_key(job);
        if !self.can_dismiss(job)
            || self.unknown_folder(job)
            || self
                .jobs
                .iter()
                .any(|j| presentation::job_key(j) == key && j.has_programs())
            || entry.status != Some(Status::Kept)
            || !entry.expected_owner_id.is_empty()
            || !zerus_client_core::creation::canonical_uuid(&entry.machine)
            || entry.row["kind"] != "linked"
        {
            return Err("This folder needs its original session or program review first.".into());
        }
        Ok(entry.clone())
    }
    fn offer_manual(
        &mut self,
        id: &str,
        original: Entry,
        data: Value,
    ) -> Result<ManualView, String> {
        if self.manual_entry(id)? != original {
            return Err(STATE_CHANGED.into());
        }
        let path = original.row["path"].as_str().ok_or(STATE_CHANGED)?;
        let rows = data["worktrees"].as_array().ok_or(STATE_CHANGED)?;
        let mut matches = rows.iter().filter(|r| r["path"] == path);
        let row = matches.next().ok_or(STATE_CHANGED)?;
        if matches.next().is_some()
            || data["state"] != "ok"
            || data["common_dir"] != original.common_dir
            || row["kind"] != "linked"
            || row["available"] != true
            || !row["ownership_id"].is_null()
            || serde_json::to_vec(row).map_err(|_| STATE_CHANGED)?.len() > 1024 * 1024
        {
            return Err(
                "This is not the same unowned registered linked worktree. Nothing was removed."
                    .into(),
            );
        }
        // Manual mode never auto-schedules: no launch owner is invented, and
        // no automatic exit-cleanup predicate is changed.
        let mut entry = original.clone();
        entry.exit_cleanup = false;
        entry.status = None;
        entry.detail.clear();
        entry.review_error.clear();
        entry.verified = false;
        let mut cleanup = Cleanup::manual(vec![entry]);
        let _ = cleanup.step();
        let step = cleanup.reviewed(&original.host, &original.common_dir, Ok(&data));
        if matches!(step, Step::Remove { .. }) {
            return Err(STATE_CHANGED.into());
        }
        let entry = &cleanup.entries[0];
        let confirmation = uuid::Uuid::new_v4().to_string();
        let view = ManualView {
            confirmation: confirmation.clone(),
            path: path.into(),
            computer: if original.host.is_empty() {
                "this computer".into()
            } else {
                original.host.clone()
            },
            branch: row["branch"].as_str().unwrap_or("—").into(),
            size: review::size(row),
            changes: row["changes"].as_u64().unwrap_or(0),
            untracked: row["untracked"].as_u64().unwrap_or(0),
            ignored: row["ignored"]
                .as_array()
                .into_iter()
                .flatten()
                .take(20)
                .filter_map(|v| v["path"].as_str().map(|p| p.chars().take(4096).collect()))
                .collect(),
            ignored_count: row["ignored_count"].as_u64().unwrap_or(0),
            ignored_size: review::data_size(row["ignored_bytes"].as_f64().unwrap_or(0.)),
            required: cleanup.required(),
            ready: entry.eligible(),
            reason: review::summary(row).join("\n"),
            sessions: row["sessions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .take(1024)
                .map(str::to_owned)
                .collect(),
            processes: row["processes"]
                .as_array()
                .into_iter()
                .flatten()
                .take(64)
                .filter_map(|p| {
                    Some(ProgramView {
                        pid: u32::try_from(p["pid"].as_u64()?).ok()?,
                        label: p["command"]
                            .as_str()
                            .unwrap_or("Program")
                            .chars()
                            .take(2000)
                            .collect(),
                    })
                })
                .collect(),
        };
        self.manual = Some(Offer {
            confirmation,
            job: id.into(),
            original,
            cleanup,
            at: Instant::now(),
        });
        Ok(view)
    }
    fn confirm_manual(
        &mut self,
        id: &str,
        token: &str,
        mut consents: Consents,
    ) -> Result<(String, review::Removal, Vec<String>), String> {
        // Consumed even on refusal. An old dialog cannot authorize another call.
        if !self
            .manual
            .as_ref()
            .is_some_and(|offer| offer.job == id && offer.confirmation == token)
        {
            return Err(STATE_CHANGED.into());
        }
        let mut offer = self.manual.take().ok_or(STATE_CHANGED)?;
        if offer.at.elapsed() >= Duration::from_secs(5 * 60)
            || self.manual_entry(id)? != offer.original
        {
            return Err(STATE_CHANGED.into());
        }
        consents.merged_branches = false;
        let Step::Remove { host, removal, .. } = offer.cleanup.remove(consents) else {
            return Err("Confirm the reviewed discard choices. Sessions and programs must stop before removal.".into());
        };
        debug_assert!(!removal.delete_branch && !removal.preserve_archive_restore);
        let key = self
            .jobs
            .iter()
            .find(|j| j.id == id)
            .map(presentation::job_key)
            .ok_or(STATE_CHANGED)?;
        let mut members = Vec::new();
        for job in self
            .jobs
            .iter_mut()
            .filter(|j| !j.dismissed && presentation::job_key(j) == key)
        {
            job.busy = true;
            job.phase = "removing";
            job.manual_review = false;
            members.push(job.id.clone());
        }
        Ok((host, removal, members))
    }
    fn manual_result(&mut self, id: &str, result: review::Removed, members: &[String]) {
        // Only the original admitted group is updated. A newly retained job at
        // this pathname cannot inherit an earlier folder's completion.
        for job in self
            .jobs
            .iter_mut()
            .filter(|j| members.contains(&j.id) && j.id != id)
        {
            job.busy = false;
            if let review::Removed::Done { reply } = &result {
                for entry in &mut job.cleanup.entries {
                    entry.status = Some(Status::Removed);
                    entry.detail = reply["branch_error"].as_str().unwrap_or("").into();
                    entry.verified = false;
                }
            }
        }
        let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) else {
            return;
        };
        let mut entry = job.cleanup.entries[0].clone();
        match result {
            review::Removed::Done { reply } => {
                entry.status = Some(Status::Removed);
                entry.detail = reply["branch_error"].as_str().unwrap_or("").into();
            }
            review::Removed::Refused { reply, detail } => {
                entry.status = Some(if reply["status"] == "kept" {
                    Status::Kept
                } else {
                    Status::Unconfirmed
                });
                entry.detail = detail;
            }
            review::Removed::Unknown { detail } => {
                entry.status = Some(Status::Unconfirmed);
                entry.detail = detail;
            }
        }
        entry.verified = false;
        // Preserve the original launch/archive identity, including its missing
        // owner. A later attempt requires a fresh explicit manual confirmation.
        job.cleanup = Cleanup::retained(vec![entry]);
        job.busy = false;
    }
}
struct Admitted {
    entry: Entry,
    host: String,
    removal: review::Removal,
    members: Vec<String>,
    view: View,
}
/// Never retain the termination model while acquiring Worktrees. The only
/// nested section follows the existing Worktrees -> termination model order.
fn admit_manual(
    inner: &Arc<Inner>,
    worktrees: &crate::worktree_cleanup::WorktreeCleanup,
    id: &str,
    confirmation: &str,
    consents: Consents,
) -> Result<Admitted, String> {
    let entry = {
        let model = lock(&inner.model);
        model
            .manual
            .as_ref()
            .filter(|offer| offer.job == id && offer.confirmation == confirmation)
            .ok_or(STATE_CHANGED)?
            .original
            .clone()
    };
    let result = worktrees.with_known_removal(
        &entry.machine,
        &entry.common_dir,
        entry.row["path"].as_str().ok_or(STATE_CHANGED)?,
        || {
            let mut model = lock(&inner.model);
            // Consumes the token and rechecks retained/overflow Unknowns, original
            // entry, group exclusion and deadline while both models are held.
            let (host, removal, members) = model.confirm_manual(id, confirmation, consents)?;
            model.revision = model.revision.wrapping_add(1);
            Ok(Admitted {
                entry: entry.clone(),
                host,
                removal,
                members,
                view: model.view(),
            })
        },
    );
    if result.is_err() {
        // with_known_removal has released Worktrees before this cleanup. Never
        // discard a newer offer that arrived during a refusal.
        let mut model = lock(&inner.model);
        if model
            .manual
            .as_ref()
            .is_some_and(|offer| offer.job == id && offer.confirmation == confirmation)
        {
            model.manual = None;
        }
    }
    result
}

#[tauri::command]
pub async fn termination_cleanup_manual_offer(
    window: tauri::Webview,
    state: State<'_, TerminationCleanup>,
    app: AppHandle,
    id: String,
) -> Result<ManualView, String> {
    force_window(&window)?;
    if crate::actions::blocked() {
        return Err(crate::actions::REPLAY_NEEDS_HGS.into());
    }
    let _read = state.0.read()?;
    let original = lock(&state.0.model).manual_entry(&id)?;
    use tauri::Manager;
    if app
        .state::<crate::worktree_cleanup::WorktreeCleanup>()
        .unknown(
            &original.machine,
            &original.common_dir,
            original.row["path"].as_str().unwrap_or(""),
        )
    {
        return Err("An unknown result will not be retried.".into());
    }
    let own = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let data = crate::worktree_cleanup::read(
            &original.host,
            &original.machine,
            &original.common_dir,
            original.row["path"].as_str().ok_or(STATE_CHANGED)?,
        )?;
        lock(&own.model).offer_manual(&id, original, data)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn termination_cleanup_manual_cancel(
    window: tauri::Webview,
    state: State<'_, TerminationCleanup>,
    confirmation: String,
) -> Result<(), String> {
    force_window(&window)?;
    let mut model = lock(&state.0.model);
    if model
        .manual
        .as_ref()
        .is_some_and(|o| o.confirmation == confirmation)
    {
        model.manual = None;
    }
    Ok(())
}
#[tauri::command]
pub fn termination_cleanup_manual_confirm(
    window: tauri::Webview,
    state: State<'_, TerminationCleanup>,
    app: AppHandle,
    id: String,
    confirmation: String,
    consents: Consents,
) -> Result<View, String> {
    force_window(&window)?;
    if crate::actions::blocked() {
        return Err(crate::actions::REPLAY_NEEDS_HGS.into());
    }
    use tauri::Manager;
    let Admitted {
        entry,
        host,
        removal,
        members,
        view,
    } = admit_manual(
        &state.0,
        app.state::<crate::worktree_cleanup::WorktreeCleanup>()
            .inner(),
        &id,
        &confirmation,
        consents,
    )?;
    let _ = app.emit("termination-cleanup-changed", ());
    let own = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = crate::worktree_cleanup::remove(&host, &entry.machine, &removal);
        let mut model = lock(&own.model);
        model.manual_result(&id, result, &members);
        changed(&app, &mut model);
    });
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> Model {
        let mut entry = Entry::terminated(
            "",
            "11111111-1111-4111-8111-111111111111",
            "/srv/example/work",
            "/srv/example/.git",
            "",
            None,
        )
        .unwrap();
        entry.status = Some(Status::Kept);
        let mut m = Model::default();
        m.jobs.push_back(Job {
            id: "original".into(),
            cleanup: Cleanup::retained(vec![entry]),
            busy: false,
            phase: "checking",
            manual_review: false,
            dismissed: false,
            program_review_pending: false,
            program_retirement: None,
            source: None,
            archive: None,
            restore_supported: false,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            programs: None,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            reservation: None,
        });
        m
    }
    fn data() -> Value {
        json!({"state":"ok","common_dir":"/srv/example/.git","worktrees":[{"path":"/srv/example/work","kind":"linked","available":true,"branch":"kept-branch","ownership_id":null,"verdict":"ready","fingerprint":"current-files","changes":0,"untracked":0}]})
    }
    fn offer(m: &mut Model, data: Value) -> ManualView {
        m.offer_manual("original", m.manual_entry("original").unwrap(), data)
            .unwrap()
    }
    #[test]
    fn unowned_manual_review_never_auto_runs_and_keeps_branch_and_original_identity() {
        let mut m = model();
        m.jobs[0].manual_review = true;
        m.jobs[0].cleanup.entries[0].verified = true;
        m.jobs[0].cleanup.entries[0].row = data()["worktrees"][0].clone();
        assert!(!m.jobs[0].has_launch_owner());
        assert!(
            !m.view().rows[0].ready,
            "unowned folders require their separate confirmation"
        );
        let original = m.jobs[0].cleanup.entries[0].clone();
        let v = offer(&mut m, data());
        assert!(v.ready);
        assert!(!m.jobs[0].busy);
        assert!(!m.manual.as_ref().unwrap().cleanup.busy());
        let (_, removal, _) = m
            .confirm_manual(
                "original",
                &v.confirmation,
                Consents {
                    merged_branches: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!removal.delete_branch && !removal.preserve_archive_restore);
        assert_eq!(removal.fingerprint, "current-files");
        assert_eq!(m.jobs[0].cleanup.entries[0], original);
        assert!(m
            .confirm_manual("original", &v.confirmation, Consents::default())
            .is_err());
        m.manual_result(
            "original",
            review::Removed::Unknown {
                detail: "lost reply".into(),
            },
            &["original".into()],
        );
        assert_eq!(
            m.jobs[0].cleanup.entries[0].status,
            Some(Status::Unconfirmed)
        );
        assert!(m.manual_entry("original").is_err());
        assert!(m.jobs[0].cleanup.entries[0].expected_owner_id.is_empty());
        assert!(m.jobs[0].cleanup.entries[0].exit_cleanup);
    }
    #[test]
    fn local_data_and_changes_need_the_exact_discard_consents() {
        for missing in 0..2 {
            let mut m = model();
            let mut d = data();
            let row = &mut d["worktrees"][0];
            row["changes"] = json!(1);
            row["untracked"] = json!(2);
            row["verdict"] = json!("blocked");
            row["discardable"] = json!(true);
            row["discard_fingerprint"] = json!("reviewed-discard");
            row["ignored_count"] = json!(1);
            row["ignored_fingerprint"] = json!("reviewed-local");
            let v = offer(&mut m, d);
            assert!(v.required.local_files && v.required.discard_changes);
            assert!(m
                .confirm_manual(
                    "original",
                    &v.confirmation,
                    Consents {
                        local_files: missing == 0,
                        discard_changes: missing == 1,
                        ..Default::default()
                    }
                )
                .is_err());
            assert!(!m.jobs[0].busy);
            assert!(m.manual.is_none());
        }
        let mut m = model();
        let mut d = data();
        let r = &mut d["worktrees"][0];
        r["discardable"] = json!(true);
        r["discard_fingerprint"] = json!("discard");
        r["ignored_count"] = json!(1);
        r["ignored_fingerprint"] = json!("local");
        let v = offer(&mut m, d);
        let (_, r, _) = m
            .confirm_manual(
                "original",
                &v.confirmation,
                Consents {
                    local_files: true,
                    discard_changes: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(r.discard_fingerprint, "discard");
        assert_eq!(r.ignored_fingerprint, "local");
        assert!(!r.delete_branch);
    }
    #[test]
    fn main_plain_aliased_replaced_and_ambiguous_targets_refuse_before_any_effect() {
        for mode in 0..6 {
            let mut m = model();
            let mut d = data();
            match mode {
                0 => d["worktrees"][0]["kind"] = json!("main"),
                1 => d["worktrees"][0]["kind"] = json!("folder"),
                2 => d["worktrees"][0]["path"] = json!("/srv/example/elsewhere"),
                3 => d["worktrees"][0]["ownership_id"] = json!("replacement-owner"),
                4 => {
                    let r = d["worktrees"][0].clone();
                    d["worktrees"].as_array_mut().unwrap().push(r);
                }
                _ => d["common_dir"] = json!("/other/.git"),
            }
            let original = m.manual_entry("original").unwrap();
            assert!(m.offer_manual("original", original, d).is_err());
            assert!(!m.jobs[0].busy);
            assert!(m.manual.is_none());
        }
    }
    #[test]
    fn fresh_session_or_program_use_cannot_be_overridden_by_discard() {
        for code in ["sessions", "processes"] {
            let mut m = model();
            let mut d = data();
            let r = &mut d["worktrees"][0];
            r["verdict"] = json!("blocked");
            r["reasons"] = json!([{"code":code,"message":"Still in use"}]);
            r[code] = json!(["known blocker"]);
            let v = offer(&mut m, d);
            assert!(!v.ready);
            assert!(m
                .confirm_manual(
                    "original",
                    &v.confirmation,
                    Consents {
                        local_files: true,
                        discard_changes: true,
                        unchecked_processes: true,
                        merged_branches: true
                    }
                )
                .is_err());
            assert!(!m.jobs[0].busy);
        }
    }
    #[test]
    fn stale_busy_pending_unknown_or_expired_confirmation_is_never_reused() {
        for mode in 0..6 {
            let mut m = model();
            let v = offer(&mut m, data());
            match mode {
                0 => m.jobs[0].busy = true,
                1 => m.jobs[0].program_review_pending = true,
                2 => m.jobs[0].cleanup.entries[0].status = Some(Status::Unconfirmed),
                3 => m.jobs[0].cleanup.entries[0].common_dir = "/replacement".into(),
                4 => m.jobs[0].dismissed = true,
                _ => m.manual.as_mut().unwrap().at = Instant::now() - Duration::from_secs(301),
            }
            assert!(m
                .confirm_manual("original", &v.confirmation, Consents::default())
                .is_err());
            assert!(m.manual.is_none());
        }
    }
    #[test]
    fn one_folder_effect_settles_all_original_sources_but_not_a_later_replacement() {
        let mut m = model();
        let mut twin = model().jobs.pop_front().unwrap();
        twin.id = "twin".into();
        m.jobs.push_back(twin);
        let v = offer(&mut m, data());
        let (_, _, members) = m
            .confirm_manual("original", &v.confirmation, Consents::default())
            .unwrap();
        assert_eq!(members.len(), 2);
        assert!(m.jobs.iter().all(|j| j.busy));
        let mut later = model().jobs.pop_front().unwrap();
        later.id = "later".into();
        m.jobs.push_back(later);
        m.manual_result(
            "original",
            review::Removed::Done {
                reply: json!({"status":"removed"}),
            },
            &members,
        );
        assert!(m.jobs.iter().all(|j| !j.busy));
        for j in &m.jobs {
            assert_eq!(
                j.cleanup.entries[0].status,
                Some(if j.id == "later" {
                    Status::Kept
                } else {
                    Status::Removed
                })
            );
        }
    }
    #[test]
    fn overflow_unknown_refuses_before_review_and_after_an_original_offer() {
        let mut initial = model();
        let original = initial.manual_entry("original").unwrap();
        initial.overflow_unknown = 1;
        assert!(initial.manual_entry("original").is_err());
        assert!(initial.offer_manual("original", original, data()).is_err());
        assert!(!initial.view().rows[0].can_remove_anyway);
        assert!(!initial.jobs[0].busy);
        assert!(initial.manual.is_none());
        let inner = Arc::new(Inner::default());
        let token = {
            let mut m = lock(&inner.model);
            *m = model();
            let v = offer(&mut m, data());
            m.overflow_unknown = 1;
            v.confirmation
        };
        assert!(admit_manual(
            &inner,
            &crate::worktree_cleanup::WorktreeCleanup::default(),
            "original",
            &token,
            Consents::default()
        )
        .is_err());
        let m = lock(&inner.model);
        assert_eq!(m.overflow_unknown, 1);
        assert!(!m.jobs[0].busy);
        assert!(m.manual.is_none());
    }
    #[test]
    fn manual_admission_waits_for_worktrees_without_retaining_the_termination_model() {
        let inner = Arc::new(Inner::default());
        let token = {
            let mut m = lock(&inner.model);
            *m = model();
            offer(&mut m, data()).confirmation
        };
        let work = crate::worktree_cleanup::WorktreeCleanup::default();
        let (started, ready) = std::sync::mpsc::sync_channel(0);
        let (finished, done) = std::sync::mpsc::sync_channel(1);
        let mut worker = None;
        let mut available = false;
        work.with_known_removal("machine", "/repo/.git", "/repo/work", || {
            let target = inner.clone();
            let other = work.clone();
            worker = Some(std::thread::spawn(move || {
                started.send(()).unwrap();
                let result = admit_manual(&target, &other, "original", &token, Consents::default());
                finished.send(()).unwrap();
                result
            }));
            ready.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(
                done.recv_timeout(Duration::from_millis(100)).is_err(),
                "admission waits for the held Worktrees model"
            );
            // Existing Worktrees completion needs this exact mutex. Probe it
            // without hanging even if a future regression restores the cycle.
            available = inner.model.try_lock().is_ok();
            Ok(())
        })
        .unwrap();
        let result = worker.unwrap().join().unwrap();
        assert!(
            available,
            "confirmation must never retain Termination while waiting for Worktrees"
        );
        assert!(result.is_ok());
        assert!(lock(&inner.model).jobs[0].busy);
    }
    #[test]
    fn a_stale_confirmation_never_consumes_a_newer_offer() {
        let mut m = model();
        let old = offer(&mut m, data());
        let new = offer(&mut m, data());
        assert!(m
            .confirm_manual("original", &old.confirmation, Consents::default())
            .is_err());
        assert_eq!(m.manual.as_ref().unwrap().confirmation, new.confirmation);
        assert!(m
            .confirm_manual("original", &new.confirmation, Consents::default())
            .is_ok());
    }
}
