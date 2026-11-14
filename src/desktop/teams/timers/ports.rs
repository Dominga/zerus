//! What the timers read from sessions and the effects they cause. Every
//! effect is this computer's own `hgs` as one original child; a read never
//! admits an effect, and an unknown result is never repeated.
use crate::teams::store::{timers::Gate, Store};
use serde::Serialize;
use zerus_team_plan::{
    model::{Member, TaskRef, Team},
    Problem,
};

pub(in crate::desktop::teams) type Result<T> = std::result::Result<T, Problem>;

/// What the service last saw of a member's exact bound session (the fleet
/// rows of the teams actor). Display data never identifies the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::desktop::teams) struct Presence {
    pub name: String,
    pub run: String,
    pub conversation: String,
    pub live: bool,
    pub working: bool,
    pub background: bool,
    pub pending: bool,
}
impl Presence {
    /// Live, idle, not Background and without a pending request.
    pub fn idle(&self) -> bool {
        self.live && !self.working && !self.background && !self.pending
    }
    /// Seen not working: neither a native turn nor Background work.
    pub fn quiet(&self) -> bool {
        !self.working && !self.background
    }
}

/// The member's current run, from the availability port of the teams actor.
pub(in crate::desktop::teams) trait Sessions {
    /// None when the service has no row for the exact bound session.
    fn presence(&mut self, team: &Team, member: &Member) -> Option<Presence>;
}

/// One reserved doorbell, the lead's or a member's: `hgs send NAME --json`
/// to the exact run and conversation it was reserved for. It names a count
/// and a command, never the text of an item, a question or an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::desktop::teams) struct Ring {
    pub id: String,
    pub gate: Gate,
    pub agent: String,
    pub name: String,
    pub run: String,
    pub conversation: String,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::desktop::teams) enum Rung {
    Submitted,
    /// Certainly not delivered: the CLI refused before any input.
    Refused(String),
    /// No child started: the admission refused or its lock was busy.
    NotStarted(String),
    /// Anything else. Never sent again for its mark.
    Uncertain(String),
}

/// The envelope of `hgs task team-clear REF --json` (plan, wave 2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(in crate::desktop::teams) struct ClearRequest {
    pub schema: u32,
    pub team: String,
    pub node: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::desktop::teams) enum Answer {
    Cleared,
    Refused(String),
    CuttingOver,
    /// No echo of the request, an unknown state, a malformed reply or a
    /// failed child: tried again on a later pass.
    None(String),
}

pub(in crate::desktop::teams) trait Effects {
    /// The doorbell. The adapter holds one immediate transaction on `store`
    /// across the spawn and calls `store::timers::admit_bell` inside it.
    fn ring(&mut self, store: &Store, ring: &Ring) -> Rung;
    /// The team-clear child for one task of a stopped or archived team.
    fn clear(&mut self, task: &TaskRef, request: &ClearRequest) -> Answer;
    /// Package 7's exact launch-ID read of a member launch.
    fn launch(
        &mut self,
        plan: &super::super::ports::Plan,
    ) -> Result<super::super::ports::LaunchRead>;
}
