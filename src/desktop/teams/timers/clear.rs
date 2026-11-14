//! After Stop or Archive the service clears the team key of each task the
//! team still marks, one small page per pass. Cleared and refused are final;
//! cutting over and no answer are tried on a later sweep, and the page moves
//! past them so later nodes never wait behind them.
use super::{
    ports::{Answer, ClearRequest, Effects, Result},
    record,
};
use crate::teams::{
    ops::{KeyClearState, Operation},
    store::Store,
};
use zerus_team_plan::model::TeamStatus;

pub(super) const PAGE: u32 = 16;

#[derive(Default)]
pub(super) struct Sweep {
    team: Option<String>,
    after_team: String,
    after_node: u32,
    /// Keys of the current team's sweep that got no final answer.
    unanswered: u32,
    /// The last of them, with its reason.
    last: Option<(u32, String)>,
    /// Teams whose sweep reached its end since the actor last took them.
    finished: Vec<Finished>,
}
/// One team's sweep that reached the end of its keys: how many got no
/// final answer (cutting over, or no answer), and the last of them. The
/// actor writes or clears the team's service item from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::desktop::teams) struct Finished {
    pub team: String,
    pub unanswered: u32,
    pub last: Option<(u32, String)>,
}
impl Sweep {
    /// The finished sweeps, taken once.
    pub(super) fn take_finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.finished)
    }
    fn miss(&mut self, node: u32, why: &str) {
        self.unanswered = self.unanswered.saturating_add(1);
        self.last = Some((node, why.into()));
    }
    fn finish(&mut self, team: String) {
        let unanswered = std::mem::take(&mut self.unanswered);
        let last = self.last.take();
        self.finished.push(Finished {
            team,
            unanswered,
            last,
        });
        self.next_team();
    }
    fn current(&mut self, store: &Store) -> Result<Option<String>> {
        if self.team.is_none() {
            let mut next = store.keyed_teams(&self.after_team, 1)?.into_iter().next();
            if next.is_none() && !self.after_team.is_empty() {
                // Wrap to the first pending team in the same pass.
                next = store.keyed_teams("", 1)?.into_iter().next();
            }
            self.after_team = next.clone().unwrap_or_default();
            self.team = next;
            self.after_node = 0;
        }
        Ok(self.team.clone())
    }
    fn next_team(&mut self) {
        self.team = None;
        self.after_node = 0;
        self.unanswered = 0;
        self.last = None;
    }
}

pub(super) fn step(
    store: &mut Store,
    effects: &mut impl Effects,
    s: &mut Sweep,
    now: i64,
) -> Result<()> {
    let Some(id) = s.current(store)? else {
        return Ok(());
    };
    let team = store.team(&id)?;
    let Some(team) =
        team.filter(|t| matches!(t.status, TeamStatus::Stopped | TeamStatus::Archived))
    else {
        s.next_team();
        return Ok(());
    };
    let page = store.keyed_tasks(&id, s.after_node, PAGE)?;
    for keyed in &page {
        s.after_node = keyed.node;
        let request = ClearRequest {
            schema: 1,
            team: id.clone(),
            node: keyed.node,
        };
        let (state, reason) = match effects.clear(&keyed.task, &request) {
            Answer::Cleared => (KeyClearState::Cleared, String::new()),
            Answer::Refused(why) => (KeyClearState::Refused, refusal(&why)),
            Answer::CuttingOver => {
                s.miss(
                    keyed.node,
                    "The task's project is being moved; the key is cleared on a later sweep.",
                );
                continue;
            }
            Answer::None(why) => {
                s.miss(keyed.node, &why);
                continue;
            }
        };
        let op = Operation::RecordKeyClear {
            node: keyed.node,
            state,
            reason,
        };
        record(store, &team, op, now)?;
    }
    if page.len() < PAGE as usize {
        store.settle_keyed(&id)?;
        s.finish(id);
    }
    Ok(())
}
fn refusal(why: &str) -> String {
    let why = why.trim();
    if why.is_empty() {
        return "The task command refused without a reason.".into();
    }
    let mut end = why.len().min(4096);
    while !why.is_char_boundary(end) {
        end -= 1;
    }
    why[..end].into()
}
