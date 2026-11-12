//! Server-only import. Public Execute still rejects Stop. The transient
//! service image/epoch is deliberately absent from the retained fact guard.
use crate::{
    state::task_controls::{Capture, Fact, Source},
    tasks::controls::wire::canonical,
};
use serde::Serialize;
use zerus_desktop_ledger::task_controls::{
    Action, Admission, Dispatch, Executor, Hold, HoldKind, Request, Store, TASK_CONTROLS,
};

const DOMAIN: &[u8] = b"zerus-task-control-stop-import-v1\0";

#[derive(Serialize)]
struct Guard<'a> {
    schema: u32,
    fact: &'a Fact,
    configuration: &'a str,
    store_identity: &'a str,
}

pub(super) fn entry(
    fact: &Fact,
    configuration: &str,
    store_identity: &str,
) -> Result<(Request, Hold), String> {
    for digest in [configuration, store_identity] {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("Unverified Stop-import configuration or store identity".into());
        }
    }
    let guard = Guard {
        schema: 1,
        fact,
        configuration,
        store_identity,
    };
    let admission = canonical::tagged_digest(
        DOMAIN,
        &serde_json::to_value(guard).map_err(|e| e.to_string())?,
    )?;
    let request = Request {
        id: fact.id.clone(),
        admission,
        action: Action::Stop,
        executor: fact.executor.clone(),
        attempt: None,
        slot: None,
        task_version: None,
        turn: fact.turn.clone(),
        observed_pauses: vec![],
        observed_stops: vec![],
    };
    request.validate().map_err(|e| e.to_string())?;
    let hold = Hold {
        id: fact.id.clone(),
        executor: fact.executor.clone(),
        kind: HoldKind::Stop,
        attempt: None,
        request_id: fact.id.clone(),
        reason: match fact.source {
            Source::PersonStop => "Explicit native Stop holds task dispatch",
            Source::VerifiedExternalInterrupt => {
                "Verified external interruption holds task dispatch"
            }
        }
        .into(),
    };
    Ok((request, hold))
}

/// The actor must call this for every effect admission after native capture
/// and lock release. Errors (including an unimportable fact) propagate before
/// dispatch is considered; successful import does not itself grant authority.
/// A final scope error may follow a durable COMMIT. It requires lookup-only
/// settlement and never proves rollback or authorizes replay.
pub(super) fn admission(
    store: &mut Store<'_>,
    captured: &Capture,
    expected: &Executor,
    configuration: &str,
    store_identity: &str,
    kind: Dispatch,
    observed_stops: &[String],
) -> Result<Admission, String> {
    if !TASK_CONTROLS {
        return Err("task_controls_inactive".into());
    }
    captured.with_current(expected, |executor, facts, check| {
        let imports = facts
            .iter()
            .map(|fact| entry(fact, configuration, store_identity))
            .collect::<Result<Vec<_>, _>>()?;
        store
            .import_stops(executor, &imports, || check().map_err(anyhow::Error::msg))
            .map_err(|e| e.to_string())?;
        // The SAME lease also spans dispatch and its actual COMMIT. A later
        // scope refusal may follow durable writes; it is not rollback proof.
        check()?;
        store
            .dispatch(executor, kind, observed_stops)
            .map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests;
