//! Folder projection and explicit result dismissal. No execution or consent.
use super::*;

pub(super) fn folder_key(entry: &Entry) -> String {
    json!([entry.machine, entry.common_dir, entry.row["path"]]).to_string()
}
pub(super) fn job_key(job: &Job) -> String {
    job.cleanup
        .entries
        .first()
        .map(folder_key)
        .unwrap_or_else(|| job.id.clone())
}
pub(super) fn folder_counts(rows: &[Row]) -> (usize, usize, usize) {
    let mut folders = std::collections::BTreeMap::new();
    for row in rows {
        let status = match row.status {
            "unknown" => 6,
            "stopping" => 5,
            "removing" => 4,
            "checking" => 3,
            "kept" | "review" => 2,
            "ready" => 1,
            _ => 0,
        };
        folders
            .entry(&row.folder_key)
            .and_modify(|old: &mut u8| *old = (*old).max(status))
            .or_insert(status);
    }
    (
        folders.values().filter(|&&s| s == 0).count(),
        folders.values().filter(|&&s| s == 2).count(),
        folders.values().filter(|&&s| s == 6).count(),
    )
}
impl Model {
    pub(super) fn unknown_folders(&self) -> BTreeSet<String> {
        self.jobs
            .iter()
            .filter(|j| {
                j.cleanup
                    .entries
                    .iter()
                    .any(|e| e.status == Some(Status::Unconfirmed))
            })
            .map(job_key)
            .collect()
    }
    pub(super) fn unknown_folder(&self, job: &Job) -> bool {
        let key = job_key(job);
        self.jobs.iter().any(|j| {
            job_key(j) == key
                && j.cleanup
                    .entries
                    .iter()
                    .any(|e| e.status == Some(Status::Unconfirmed))
        })
    }
    pub(super) fn clearable(&self, job: &Job) -> bool {
        if job.dismissed || !job.clearable() {
            return false;
        }
        let key = job_key(job);
        !self
            .jobs
            .iter()
            .any(|j| !j.dismissed && job_key(j) == key && !j.clearable())
    }
    pub(super) fn can_dismiss(&self, job: &Job) -> bool {
        if job.dismissed || !self.restoring.is_empty() {
            return false;
        }
        let key = job_key(job);
        !self
            .jobs
            .iter()
            .any(|j| !j.dismissed && job_key(j) == key && !j.dismissible())
    }
    pub(super) fn prune_dismissed(&mut self) {
        self.jobs.retain(|job| {
            !job.dismissed
                || job.busy
                || job.has_programs()
                || job
                    .cleanup
                    .entries
                    .iter()
                    .any(|entry| entry.status == Some(Status::Unconfirmed))
        });
    }
    pub(super) fn clear_settled_results(&mut self) {
        let cleared: BTreeSet<_> = self
            .jobs
            .iter()
            .filter(|j| self.clearable(j))
            .map(|j| j.id.clone())
            .collect();
        for job in &mut self.jobs {
            if cleared.contains(&job.id) && job.has_programs() {
                job.dismissed = true;
            }
        }
        self.jobs
            .retain(|j| !cleared.contains(&j.id) || j.dismissed);
        self.overflow_kept = 0;
    }
    pub(super) fn dismiss(&mut self, id: &str) -> Result<(), String> {
        let job = self
            .jobs
            .iter()
            .find(|j| j.id == id && !j.dismissed)
            .ok_or(STATE_CHANGED)?;
        if !self.can_dismiss(job) {
            return Err("This folder still has unfinished work. Its review was kept.".into());
        }
        let key = job_key(job);
        if job
            .cleanup
            .entries
            .iter()
            .any(|e| e.status == Some(Status::Unconfirmed))
        {
            self.jobs
                .iter_mut()
                .find(|j| j.id == id)
                .ok_or(STATE_CHANGED)?
                .dismissed = true;
        } else {
            if self.jobs.iter().any(|j| {
                !j.dismissed
                    && job_key(j) == key
                    && j.cleanup
                        .entries
                        .iter()
                        .any(|e| e.status == Some(Status::Unconfirmed))
            }) {
                return Err("Dismiss unknown results individually first.".into());
            }
            for job in &mut self.jobs {
                if !job.dismissed && job_key(job) == key && job.has_programs() {
                    job.dismissed = true;
                }
            }
            self.jobs.retain(|j| j.dismissed || job_key(j) != key);
        }
        Ok(())
    }
}
