//! Distinct current native child/reservation admission. The SAME still-live
//! original Remove continuation remains required in the parent; a stop permit,
//! stored Complete report, holder or this registration cannot reconstruct it.
use super::{store::Location, ui_authority, Config};
use anyhow::Result;
use serde_json::Value;
use std::time::Instant;
use zerus_client_core::worktree_programs::cleanup::Intent;

pub(crate) use ui_authority::ProgramCleanupPermit;
pub(crate) struct Proof {
    original: ui_authority::ProgramCleanupAdmission,
    location: Location,
}
impl Proof {
    pub(crate) fn capture(config: &Config, envelope: &Value) -> Result<Self> {
        let location = Location::new(config)?;
        let scope = ui_authority::Scope::new(config, &location);
        let original = ui_authority::authenticate_program_cleanup(&scope, envelope)?
            .into_program_cleanup(&mut ui_authority::Order::default())?;
        Ok(Self { original, location })
    }
    pub(crate) fn consume<'a>(
        self,
        intent: &Intent,
        until: Instant,
        claim: &'a crate::state::ProgramRemovalClaim,
    ) -> Result<ProgramCleanupPermit<'a>> {
        self.original.consume(intent, until, &self.location, claim)
    }
}
