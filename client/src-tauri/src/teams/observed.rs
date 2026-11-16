//! Read-only follow, on the existing single Teams poller.
use super::*;
#[tauri::command]
pub fn teams_observed(teams:State<'_,Teams>,team:String,phase:u32,node:Option<u32>,window_ms:u64,enabled:bool,expected_epoch:Option<u64>)->Result<u64,String> {
    if !valid_team(&team)||!(1..=zerus_client_core::teams::observed_timing::MAX_WINDOW_MS).contains(&window_ms) {return Err("Not an observation scope.".into())}
    teams.poller().as_ref().and_then(|p|p.observed(&team,phase,node,window_ms,enabled,expected_epoch)).ok_or_else(||"This phase is not followed.".into())
}
