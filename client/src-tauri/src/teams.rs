//! The Teams views' reads (task #6): one core [`TeamsPoller`] per computer a
//! window follows, feeding the pump. The Team tab, the plan graph, Waiting
//! and New team get their projected views in the per-frame batches
//! (`batch.teams`, keyed `list`, `waiting`, `team:<id>`, ...); the webview
//! never runs or parses an `hgs team` read. A replay never runs `hgs`.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::State;
use zerus_client_core::fleet::valid_alias;
use zerus_client_core::teams::read::valid_team;
use zerus_client_core::teams::Heads;
use zerus_client_core::{default_hgs, ComputerId, TeamsFollow, TeamsPoller};

use crate::commands::AppState;

/// Computers whose Teams views are followed at once.
const COMPUTERS: usize = 16;

/// The pollers by computer.
pub struct Teams {
    pollers: Mutex<BTreeMap<ComputerId, TeamsPoller>>,
    /// Task 8: each computer's latest stream team heads, for a poller that
    /// starts later; None or absent: it polls.
    heads: Mutex<BTreeMap<ComputerId, Arc<Heads>>>,
    replay: bool,
}

impl Teams {
    pub fn new(replay: bool) -> Self {
        Self {
            pollers: Mutex::new(BTreeMap::new()),
            heads: Mutex::new(BTreeMap::new()),
            replay,
        }
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<ComputerId, TeamsPoller>> {
        self.pollers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Follows `follow` on `computer`; following nothing stops its poller.
    pub fn follow(
        &self,
        pump: &crate::pump::Pump,
        computer: ComputerId,
        follow: TeamsFollow,
    ) -> Result<(), String> {
        if self.replay {
            return Err("Teams need hgs; a replay has none.".into());
        }
        let mut pollers = self.lock();
        if follow == TeamsFollow::default() {
            if let Some(poller) = pollers.get(&computer) {
                // Its views end through the batch; then the thread stops.
                poller.follow(follow);
            }
            pollers.remove(&computer);
            return Ok(());
        }
        if !pollers.contains_key(&computer) {
            if pollers.len() >= COMPUTERS {
                return Err("Too many computers follow their teams at once.".into());
            }
            let pump = pump.clone();
            let poller = TeamsPoller::spawn(default_hgs(), computer.clone(), move |change| {
                pump.push(change)
            })
            .map_err(|error| format!("The Teams reader did not start: {error}"))?;
            if let Some(heads) = self
                .heads
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&computer)
            {
                poller.heads(Some(Arc::clone(heads)));
            }
            pollers.insert(computer.clone(), poller);
        }
        if let Some(poller) = pollers.get(&computer) {
            poller.follow(follow);
        }
        Ok(())
    }

    /// Task 8: the team heads of a computer's watch stream (its `team`
    /// topic), or None while it gives none. With heads its poller reads a
    /// team again when its revision moved instead of every 3 s.
    pub fn heads(&self, computer: &ComputerId, heads: Option<Arc<Heads>>) {
        {
            let mut known = self.heads.lock().unwrap_or_else(PoisonError::into_inner);
            match &heads {
                Some(heads) => {
                    known.insert(computer.clone(), Arc::clone(heads));
                }
                None => {
                    known.remove(computer);
                }
            }
        }
        if let Some(poller) = self.lock().get(computer) {
            poller.heads(heads);
        }
    }

    fn with(&self, computer: &ComputerId, ask: impl FnOnce(&TeamsPoller) -> bool) -> bool {
        self.lock().get(computer).is_some_and(ask)
    }
}

/// The computer a command names: empty is this computer.
fn computer(alias: &str) -> Result<ComputerId, String> {
    if alias.is_empty() {
        return Ok(ComputerId::local());
    }
    if !valid_alias(alias) {
        return Err("Not a computer alias.".into());
    }
    Ok(ComputerId::from(alias))
}

/// Follows the Teams views of one computer: the team list, Waiting and at
/// most one team (its tab, plan and New team form). All false and no team
/// stops reading that computer.
#[tauri::command]
pub fn teams_follow(
    state: State<'_, AppState>,
    teams: State<'_, Teams>,
    computer: String,
    list: bool,
    waiting: bool,
    team: Option<String>,
) -> Result<(), String> {
    let team = team.filter(|team| !team.is_empty());
    if team.as_deref().is_some_and(|team| !valid_team(team)) {
        return Err("Not a team id.".into());
    }
    let target = self::computer(&computer)?;
    teams.follow(
        &state.pump(),
        target,
        TeamsFollow {
            list,
            waiting,
            team,
        },
    )
}

/// Loads the watched team's feed page before the oldest event shown. False:
/// there is none, the feed is at its cap, or a page is already asked for.
#[tauri::command]
pub fn teams_older(teams: State<'_, Teams>, computer: String) -> Result<bool, String> {
    let target = self::computer(&computer)?;
    Ok(teams.with(&target, TeamsPoller::older))
}

/// Reads the watched team's time spans after task `after`.
#[tauri::command]
pub fn teams_spans(teams: State<'_, Teams>, computer: String, after: u64) -> Result<bool, String> {
    let target = self::computer(&computer)?;
    Ok(teams.with(&target, |poller| poller.spans(after)))
}

/// Reads the watched team's profile.
#[tauri::command]
pub fn teams_profile(teams: State<'_, Teams>, computer: String) -> Result<bool, String> {
    let target = self::computer(&computer)?;
    Ok(teams.with(&target, TeamsPoller::profile))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_name_a_real_computer_and_a_replay_runs_no_hgs() {
        assert_eq!(computer("").unwrap(), ComputerId::local());
        assert_eq!(computer("studio").unwrap(), ComputerId::from("studio"));
        assert!(computer("-oProxyCommand=x").is_err());
        let teams = Teams::new(true);
        let pump = crate::pump::Pump::default();
        let follow = TeamsFollow {
            list: true,
            ..TeamsFollow::default()
        };
        assert!(teams.follow(&pump, ComputerId::local(), follow).is_err());
        assert!(teams.lock().is_empty());
        // Following nothing on a computer that has no poller changes nothing.
        let live = Teams::new(false);
        live.follow(&pump, ComputerId::local(), TeamsFollow::default())
            .unwrap();
        assert!(live.lock().is_empty());
        assert!(!live.with(&ComputerId::local(), TeamsPoller::older));
    }
}
