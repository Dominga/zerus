//! Production ports. The service's original hgs child performs ordinary CLI
//! creation; it does not borrow a person's UI-gesture admission. Git plumbing
//! for the integration ref stays in teams::integrate.
mod arguments;
mod observation;
#[cfg(test)]
mod tests;
use super::{
    child::{self, Ended},
    launch::{invalid, uncertain},
    ledger::BranchPlan,
    ports::*,
};
use crate::{config::Config, teams::integrate};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use zerus_team_plan::{
    accounts::{AccountUsage, UsageWindow},
    problem::code,
};

pub(super) struct Native {
    pub config: Config,
    pub machine: String,
    pub stop: Arc<AtomicBool>,
}
impl Native {
    fn read(&self, args: &[String], input: Option<&[u8]>) -> Result<Value> {
        self.read_until(args, input, Instant::now() + Duration::from_secs(15))
    }
    fn read_until(
        &self,
        args: &[String],
        input: Option<&[u8]>,
        deadline: Instant,
    ) -> Result<Value> {
        match child::run(&self.config, args, input, None, false, deadline, &self.stop) {
            Ended::Exited { code: 0, output } => {
                serde_json::from_slice(&output).map_err(|_| invalid("Invalid bounded native read"))
            }
            _ => Err(invalid("Native read is unavailable")),
        }
    }
    fn project(&self, project: &str) -> Result<PathBuf> {
        // Reuse the existing bounded, lock-free catalog projection, on this
        // worker only. Never bootstrap a catalog or choose an arbitrary cwd.
        let snapshot = crate::swarm::organization(&self.config, false)
            .map_err(|_| invalid("Project catalog unavailable"))?;
        if snapshot["node_id"] != self.machine {
            return Err(invalid("Project computer identity changed"));
        }
        let mut found = std::collections::BTreeSet::new();
        let rows = snapshot["organization"]["projects"]
            .as_array()
            .ok_or_else(|| invalid("Project catalog unavailable"))?;
        for p in rows.iter().filter(|p| p["id"] == project) {
            for f in p["folders"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|f| f["machine_id"] == self.machine)
            {
                if let Some(path) = f["path"].as_str() {
                    found.insert(PathBuf::from(path));
                }
            }
        }
        if found.len() != 1 {
            return Err(invalid(
                "Team project needs exactly one local repository folder",
            ));
        }
        let source = crate::platform::canonical(found.into_iter().next().expect("one folder"))
            .map_err(|_| invalid("Project folder unavailable"))?;
        if !source.is_absolute() {
            return Err(invalid("Project folder is not absolute"));
        }
        Ok(source)
    }
}
impl Effects for Native {
    fn hosted_ready(&mut self, plan: &Plan, bound: bool) -> Result<()> {
        if plan.agent != "dsh" { return Ok(()); }
        let (agent, home) = crate::accounts::hook_profile(&self.config, &plan.account)
            .map_err(|e| invalid(&e.message))?;
        if agent != "dsh" { return Err(invalid("DeepSeek account identity changed")); }
        let home = home.to_str().ok_or_else(|| invalid("DeepSeek account path is not UTF-8"))?;
        match crate::teams::hosted::deepseek_host(home)? {
            crate::teams::hosted::HostState::Proves => Ok(()),
            crate::teams::hosted::HostState::NoHost if !bound => Ok(()),
            crate::teams::hosted::HostState::NoHost => Err(invalid("The new DeepSeek host is unavailable; the member was not bound")),
            _ => Err(crate::teams::hosted::older(home)),
        }
    }
    fn placement(&mut self, project: &str, launch: &str) -> Result<Placement> {
        let source = self.project(project)?;
        let git_dir = observation::common_dir(&source)?;
        let destination = source
            .parent()
            .ok_or_else(|| invalid("Repository has no worktree parent"))?
            .join(format!("zerus-team-{launch}"));
        Ok(Placement {
            source: source
                .to_str()
                .ok_or_else(|| invalid("Project path is not UTF-8"))?
                .into(),
            git_dir: git_dir
                .to_str()
                .ok_or_else(|| invalid("Repository path is not UTF-8"))?
                .into(),
            branch: format!("zerus/team/member-{launch}"),
            destination: destination
                .to_str()
                .ok_or_else(|| invalid("Worktree path is not UTF-8"))?
                .into(),
        })
    }
    fn branch_exists(&mut self, plan: &BranchPlan) -> Result<bool> {
        integrate::branch_exists(
            &integrate::Repo {
                git_dir: PathBuf::from(&plan.placement.git_dir),
            },
            &plan.branch,
        )
    }
    fn create_branch(&mut self, plan: &BranchPlan) -> Effect<()> {
        if self.stop.load(Ordering::Acquire) || !crate::teams::enabled(&self.config.state) {
            return Effect::NotStarted;
        }
        let admitted = (|| -> Result<bool> {
            let store = crate::teams::store::Store::open(&self.config.state)?;
            let team = store.team(&plan.team)?;
            Ok(team.is_some_and(|t| {
                t.status == zerus_team_plan::model::TeamStatus::Running
                    && t.home == plan.machine
                    && t.project == plan.project
                    && t.integration_branch == plan.branch
                    && t.base_commit == plan.base
            }))
        })();
        if admitted != Ok(true) {
            return Effect::NotStarted;
        }
        match integrate::create_branch(
            &integrate::Repo {
                git_dir: PathBuf::from(&plan.placement.git_dir),
            },
            &plan.branch,
            &plan.base,
        ) {
            Ok(()) => Effect::Submitted(()),
            Err(p) if p.code == code::DELIVERY_UNCERTAIN => Effect::Unknown(p),
            Err(p) => Effect::Refused(p),
        }
    }
    fn accounts(&mut self, agent: &str) -> Result<Vec<AccountUsage>> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let list = self.read_until(&["account".into(), "ls".into()], None, deadline)?;
        let profiles = list["profiles"]
            .as_array()
            .ok_or_else(|| invalid("Account catalog unavailable"))?;
        if profiles.len() > 200 {
            return Err(invalid("Account catalog exceeds its bound"));
        }
        let mut accounts = Vec::new();
        for row in profiles.iter().filter(|r| r["provider"] == agent) {
            let id = row["id"]
                .as_str()
                .ok_or_else(|| invalid("Account has no identity"))?;
            let data = self.read_until(
                &["account".into(), "inspect".into(), id.into()],
                None,
                deadline,
            )?;
            accounts.push(arguments::usage(&data, agent, id)?);
        }
        Ok(accounts)
    }
    fn worktree(&mut self, plan: &Plan, original: &str) -> Effect<Option<Worktree>> {
        let args = arguments::worktree(plan);
        match child::run(
            &self.config,
            &args,
            None,
            Some(child::Original {
                id: original,
                plan,
                bound: false,
            }),
            false,
            Instant::now() + Duration::from_secs(330),
            &self.stop,
        ) {
            Ended::NotStarted => Effect::NotStarted,
            Ended::Unknown(reason) => Effect::Unknown(uncertain(reason)),
            Ended::Exited { code: 0, output } => {
                let result = (|| {
                    let reply: Value = serde_json::from_slice(&output)
                        .map_err(|_| uncertain("Worktree creation reply unavailable"))?;
                    if reply["status"] != "created"
                        || reply["path"] != plan.placement.destination
                        || reply["branch"] != plan.placement.branch
                        || reply["managed_cleanup"] != true
                    {
                        return Err(uncertain("Worktree creation ownership unconfirmed"));
                    }
                    observation::ownership(&self.config, &plan.placement)
                })();
                match result {
                    Ok(tree) => Effect::Submitted(Some(tree)),
                    Err(p) if p.code != code::DELIVERY_UNCERTAIN => Effect::Submitted(None),
                    Err(p) => Effect::Unknown(p),
                }
            }
            Ended::Exited { code, .. } => Effect::Refused(invalid(&format!(
                "Worktree creation refused with exit {code}; retained files require review"
            ))),
        }
    }
    fn verify_worktree(&mut self, plan: &Plan) -> Result<Worktree> {
        observation::ownership(&self.config, &plan.placement)
    }
    fn launch(&mut self, plan: &Plan, worktree: &Worktree, original: &str) -> Effect<()> {
        let args = match arguments::launch(plan, worktree) {
            Ok(args) => args,
            Err(p) => return Effect::Refused(p),
        };
        match observation::ownership(&self.config, &plan.placement) {
            Ok(current) if &current == worktree => (),
            Ok(_) => return Effect::Refused(uncertain("Generated worktree ownership changed")),
            Err(p) if p.code == code::DELIVERY_UNCERTAIN => return Effect::Refused(p),
            Err(p) => return Effect::Deferred(p),
        }
        match child::run(
            &self.config,
            &args,
            None,
            Some(child::Original {
                id: original,
                plan,
                bound: false,
            }),
            true,
            Instant::now() + Duration::from_secs(90),
            &self.stop,
        ) {
            Ended::NotStarted => Effect::NotStarted,
            Ended::Unknown(reason) => Effect::Unknown(uncertain(reason)),
            Ended::Exited { code: 0, .. } => Effect::Submitted(()),
            Ended::Exited { code, output } => {
                let reason = String::from_utf8_lossy(&output);
                Effect::Refused(invalid(&format!(
                    "Launch refused with exit {code}: {}",
                    reason.chars().take(4096).collect::<String>()
                )))
            }
        }
    }
    fn observe(&mut self, plan: &Plan) -> Result<LaunchRead> {
        observation::session(self, plan, Instant::now() + Duration::from_secs(15))
    }
    fn parent(&mut self, plan: &Plan, _session: &ObservedSession, original: &str) -> String {
        if plan.agent == "opencode" {
            return "unsupported_agent".into();
        }
        let result = (|| -> Result<String> {
            // Both sessions through their durable handles (zerus-63rru.53):
            // a lead whose first run ended, or launched over a week ago, has
            // no launch evidence left but still links.
            let store = crate::teams::store::Store::open(&self.config.state)?;
            let current = match observation::member_session(
                self,
                &store,
                plan,
                Instant::now() + Duration::from_secs(3),
            )? {
                LaunchRead::Session(s) => *s,
                _ => return Ok("identity_pending".into()),
            };
            if current.run_id.is_empty() || current.session_id.is_empty() {
                return Ok("identity_pending".into());
            }
            let team = store
                .team(&plan.team)?
                .ok_or_else(|| invalid("Team disappeared"))?;
            let lead_member = store
                .member(&plan.team, &team.lead)?
                .ok_or_else(|| invalid("Lead disappeared"))?;
            if lead_member.session != plan.parent {
                return Ok("changed_lead".into());
            }
            let lead_plan = super::ledger::Ledger::open(&self.config.state)?
                .get(&plan.team, &team.lead)?
                .and_then(|r| r.plan)
                .ok_or_else(|| invalid("Lead original unavailable"))?;
            let lead = match observation::member_session(
                self,
                &store,
                &lead_plan,
                Instant::now() + Duration::from_secs(3),
            )? {
                LaunchRead::Session(s) => *s,
                _ => return Ok("identity_pending".into()),
            };
            if lead.run_id.is_empty()
                || lead.conversation_id.is_empty()
                || lead.session_id.is_empty()
            {
                return Ok("identity_pending".into());
            }
            let mut request = json!({"expected_run_id":current.run_id,"expected_conversation_id":current.conversation_id,"action":"read"});
            let args = vec!["parent".into(), current.name.clone(), "--json".into()];
            let read = self.read(
                &args,
                Some(
                    &serde_json::to_vec(&request)
                        .map_err(|_| invalid("Parent read unavailable"))?,
                ),
            )?;
            if !read["parent"].is_null() {
                return Ok("existing_link_kept".into());
            }
            request["action"] = json!("set");
            request["parent"] = json!({"session_id":lead.session_id,"name":lead.name,"run_id":lead.run_id,"conversation_id":lead.conversation_id});
            let bytes =
                serde_json::to_vec(&request).map_err(|_| invalid("Parent request unavailable"))?;
            match child::run(
                &self.config,
                &args,
                Some(&bytes),
                Some(child::Original {
                    id: original,
                    plan,
                    bound: true,
                }),
                false,
                Instant::now() + Duration::from_secs(15),
                &self.stop,
            ) {
                Ended::Exited { code: 0, .. } => Ok("manual".into()),
                Ended::NotStarted => Ok("not_started".into()),
                Ended::Exited { .. } => Ok("refused".into()),
                Ended::Unknown(_) => Ok("unknown".into()),
            }
        })();
        result.unwrap_or_else(|_| "identity_pending".into())
    }
}
