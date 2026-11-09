//! Closed own-helper admission only. This is not a renderer route, service
//! executor, or permission reconstructed from a durable stop receipt.
use super::{store::Location, ui_authority, Config};
use anyhow::Result;
use serde_json::Value;
use std::time::Instant;
use zerus_client_core::worktree_programs::invocation::Intent;

pub(crate) use ui_authority::ProgramStopPermit;
pub(crate) struct Proof {
    original: ui_authority::ProgramStopAdmission,
    location: Location,
}
impl Proof {
    pub(crate) fn capture(config: &Config, envelope: &Value) -> Result<Self> {
        let location = Location::new(config)?;
        let scope = ui_authority::Scope::new(config, &location);
        let original = ui_authority::authenticate_program_stop(&scope, envelope)?
            .into_program_stop(&mut ui_authority::Order::default())?;
        Ok(Self { original, location })
    }
    pub(crate) fn consume<'a>(
        self,
        intent: &Intent,
        until: Instant,
        claim: &'a crate::state::ProgramStopClaim,
    ) -> Result<ProgramStopPermit<'a>> {
        self.original.consume(intent, until, &self.location, claim)
    }
}
