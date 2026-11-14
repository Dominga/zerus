//! Git adapters run on the owned worker; integrate owns the bounded Git children.
use super::native::Native;
use super::*;
use crate::teams::integrate;
use ports::{MergePlan, MergeResult};
use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
#[cfg(test)]
thread_local! {
    pub(super) static TEST_REPO: std::cell::RefCell<Option<String>> = const {std::cell::RefCell::new(None)};
    pub(super) static AFTER_REPORTED: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {std::cell::RefCell::new(None)};
}
impl Native {
    pub(super) fn work_repository(&self, team: &Team) -> Result<String> {
        if self.stop.load(Ordering::Acquire) {
            return Err(problem("Integration read stopped"));
        }
        #[cfg(test)]
        if let Some(repo) = TEST_REPO.with(|s| s.borrow().clone()) {
            return Ok(repo);
        }
        let mut launches = crate::desktop::teams::native::Native {
            config: self.config.clone(),
            machine: self.machine.clone(),
            stop: self.stop.clone(),
        };
        // The maintained placement read selects exactly one local project and
        // resolves its common Git directory. It creates no worktree or branch.
        let placement = crate::desktop::teams::ports::Effects::placement(
            &mut launches,
            &team.project,
            &uuid::Uuid::new_v4().to_string(),
        )?;
        Ok(placement.git_dir)
    }
    pub(super) fn work_reported(
        &self,
        team: &Team,
        member: &Member,
    ) -> Result<integrate::Reported> {
        if self.stop.load(Ordering::Acquire) {
            return Err(problem("Report read stopped"));
        }
        let repo = self.work_repository(team)?;
        let owned = || {
            crate::state::team_worktree_ownership(
                &self.config.state,
                Path::new(&repo),
                Path::new(&member.worktree),
            )
            .map_err(|_| problem("Worktree ownership read is unavailable"))?
            .ok_or_else(|| problem("The report worktree is not owned"))
        };
        let before = owned()?;
        if member.ownership_nonce.is_empty() || before["id"] != member.ownership_nonce {
            return Err(problem("Report worktree ownership changed"));
        }
        if self.stop.load(Ordering::Acquire) {
            return Err(problem("Report read stopped"));
        }
        let head = integrate::reported_commit(Path::new(&member.worktree))?;
        #[cfg(test)]
        if let Some(hook) = AFTER_REPORTED.with(|s| s.borrow_mut().take()) {
            hook();
        }
        let after = owned()?;
        if after["id"] != member.ownership_nonce {
            return Err(problem("Report worktree ownership changed"));
        }
        Ok(head)
    }
    pub(super) fn work_descends(&self, plan: &MergePlan) -> Result<bool> {
        if self.stop.load(Ordering::Acquire) {
            return Err(problem("Integration read stopped"));
        }
        integrate::descends(
            &integrate::Repo {
                git_dir: PathBuf::from(&plan.repo),
            },
            &plan.commit,
            &plan.base,
        )
    }
    pub(super) fn work_contains(&self, plan: &MergePlan) -> Result<Option<String>> {
        if self.stop.load(Ordering::Acquire) {
            return Err(problem("Integration read stopped"));
        }
        let store = Store::open(&self.config.state)?;
        let team = store
            .team(&plan.team)?
            .ok_or_else(|| problem("Integration team disappeared"))?;
        if team.home != plan.machine
            || team.project != plan.project
            || team.integration_branch != plan.branch
            || self.work_repository(&team)? != plan.repo
        {
            return Err(problem("Integration read placement changed"));
        }
        let reference = format!(
            "refs/heads/{}",
            plan.branch
                .strip_prefix("refs/heads/")
                .unwrap_or(&plan.branch)
        );
        integrate::containing_tip(
            &integrate::Repo {
                git_dir: PathBuf::from(&plan.repo),
            },
            &reference,
            &plan.commit,
        )
    }
    pub(super) fn work_merge(&self, plan: &MergePlan) -> MergeResult {
        if self.stop.load(Ordering::Acquire) || !crate::teams::enabled(&self.config.state) {
            return MergeResult::Unavailable {
                reason: "Integration did not start".into(),
            };
        }
        if let Err(error) = self.merge_admission(plan) {
            return MergeResult::Unavailable {
                reason: cut(&error.message, 2048),
            };
        }
        if self.stop.load(Ordering::Acquire) {
            return MergeResult::Unavailable {
                reason: "Integration did not start".into(),
            };
        }
        let message = format!("Zerus team {} accept {}", plan.team, plan.decision);
        match integrate::merge_admitted(
            &integrate::Repo {
                git_dir: PathBuf::from(&plan.repo),
            },
            &plan.branch,
            &plan.commit,
            &message,
            || self.merge_admission(plan),
        ) {
            Ok(integrate::Merge::Merged { commit }) => MergeResult::Merged { commit },
            Ok(integrate::Merge::Conflict { files }) => MergeResult::Conflict { files },
            Ok(integrate::Merge::Stale) => MergeResult::Unavailable {
                reason: "Integration branch changed before the write".into(),
            },
            Err(error) if error.code == code::DELIVERY_UNCERTAIN => MergeResult::Unknown {
                reason: cut(&error.message, 2048),
            },
            Err(error) => MergeResult::Unavailable {
                reason: cut(&error.message, 2048),
            },
        }
    }
    fn merge_admission(
        &self,
        plan: &MergePlan,
    ) -> Result<crate::teams::store::actor_reads::Admission> {
        let store = Store::open(&self.config.state)?.admission()?;
        let team = store
            .team(&plan.team)?
            .ok_or_else(|| problem("Integration team disappeared"))?;
        if team.status != TeamStatus::Running
            || team.home != self.machine
            || team.home != plan.machine
            || team.owner != plan.owner
            || team.project != plan.project
            || team.integration_branch != plan.branch
            || team.base_commit != plan.base
            || team.settings.integration != Integration::Branch
        {
            return Err(problem("Integration admission changed"));
        }
        if store.generation(&plan.team, plan.node)? != Some(plan.generation) {
            return Err(problem("Integration generation changed"));
        }
        if !store.accepted_task_current(&plan.team, plan.node, plan.generation)? {
            return Err(problem("The accepted task's reviewed attempt changed"));
        }
        let report = store
            .report(&plan.team, plan.node, plan.generation)?
            .ok_or_else(|| problem("Pinned report disappeared"))?;
        if report.commit != plan.commit || report.member != plan.member {
            return Err(problem("Pinned report changed"));
        }
        let mut cursor = None;
        for _ in 0..4 {
            let page = store.integration_accepts(&plan.team, 200, cursor.as_ref())?;
            if page.rows.iter().any(|d| {
                d.seq == plan.decision && d.node == plan.node && d.generation == plan.generation
            }) {
                if self.work_repository(&team)? != plan.repo {
                    return Err(problem("Integration placement changed"));
                }
                if self.stop.load(Ordering::Acquire) || !crate::teams::enabled(&self.config.state) {
                    return Err(problem("Integration did not start"));
                }
                return Ok(store);
            }
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        Err(problem("The exact Ready acceptance is no longer current"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::teams::ops::tests::*;
    use std::sync::{atomic::AtomicBool, Arc};
    fn native(h: &Harness) -> Native {
        Native {
            config: Config {
                home: String::new(),
                dir: h.temp.path().join("config"),
                state: h.temp.path().to_owned(),
                selves: vec![],
                peers: vec![],
                base_peers: vec![],
                peers_from_environment: false,
                machines: Default::default(),
                tab: false,
                tab_colors: String::new(),
            },
            machine: h.store.team(TEAM).unwrap().unwrap().home,
            epoch: "fixture".into(),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
    fn plan(h: &Harness, node: u32) -> MergePlan {
        let t = h.store.team(TEAM).unwrap().unwrap();
        MergePlan {
            team: TEAM.into(),
            owner: t.owner,
            machine: t.home,
            project: t.project,
            branch: t.integration_branch,
            base: t.base_commit,
            repo: h
                .temp
                .path()
                .join("missing-repo")
                .to_string_lossy()
                .into_owned(),
            node,
            generation: 1,
            decision: h.accept_decision(node),
            member: "m1".into(),
            commit: COMMIT.into(),
            id: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn refused(reply: MergeResult, expected: &str) {
        assert!(matches!(reply,MergeResult::Unavailable{reason} if reason.contains(expected)));
    }
    #[test]
    fn merge_stop_and_exact_admission_are_verified_before_repository_or_git_access() {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let n = native(&h);
        let p = plan(&h, node);
        n.stop.store(true, Ordering::Release);
        refused(n.work_merge(&p), "did not start");
        n.stop.store(false, Ordering::Release);
        let mut wrong = p.clone();
        wrong.generation += 1;
        refused(n.work_merge(&wrong), "generation changed");
        wrong = p.clone();
        wrong.commit = "1123456789abcdef0123456789abcdef01234567".into();
        refused(n.work_merge(&wrong), "Pinned report changed");
        wrong = p.clone();
        wrong.decision += 1;
        refused(n.work_merge(&wrong), "exact Ready acceptance");
        wrong = p.clone();
        wrong.base = "1123456789abcdef0123456789abcdef01234567".into();
        refused(n.work_merge(&wrong), "admission changed");
        wrong = p.clone();
        wrong.machine = "other-machine".into();
        refused(n.work_merge(&wrong), "admission changed");
        h.run(&person(), Operation::Pause).unwrap();
        refused(n.work_merge(&p), "admission changed");
        assert!(!Path::new(&p.repo).exists());
    }
}
