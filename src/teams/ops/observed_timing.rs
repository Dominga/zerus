//! Optional observations have their OWN common-kernel transaction/receipt.
//! Canonical task/delivery/lifecycle writes never depend on this companion.
use super::*;
use crate::teams::store::observed_timing as data;
use serde::{Deserialize, Serialize};
use zerus_team_plan::observed_timing::*;
mod sample;
pub(crate) use sample::TaskTimingSample;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimingInput {
    Service {
        task: TaskRef,
        node_generation: u32,
        observer_epoch: String,
        source_request: String,
        source_sequence: u64,
    },
    Task {
        sample: Box<TaskTimingSample>,
    },
}
pub(super) enum Decision {
    Unchanged,
    Append(Box<TimingFact>),
}

pub(super) fn prepare(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::RecordTimingFact { node, observation } = ctx.op else {
        unreachable!()
    };
    ctx.timing = Some(match observation.as_ref() {
        TimingInput::Service {
            task,
            node_generation,
            observer_epoch,
            source_request,
            source_sequence,
        } => {
            uuid(observer_epoch)?;
            let n = ctx.node(*node)?;
            if n.task.as_ref() != Some(task) || ctx.generation(*node)? != *node_generation {
                return Err(invalid("The selected observation task/generation changed."));
            }
            // This schema/index work belongs only to the admitted observation
            // operation. A failure rolls back ONLY that request, not its source.
            data::extend(ctx.db)?;
            service(
                ctx,
                *node,
                task,
                *node_generation,
                observer_epoch,
                source_request,
                *source_sequence,
            )?
        }
        TimingInput::Task { sample } => {
            if sample::unchanged(ctx, *node, sample)? {
                Decision::Unchanged
            } else {
                Decision::Append(Box::new(sample::build(ctx, *node, sample)?))
            }
        }
    });
    Ok(())
}
pub(super) fn persist(ctx: &Context<'_>, seq: u64, mut fact: TimingFact) -> Result<()> {
    data::extend(ctx.db)?;
    fact.seq = seq;
    data::append(ctx.db, &fact)
}

fn original(db: &Connection, team: &str, id: &str, seq: u64) -> Result<(Request, u64, i64)> {
    // Real source sequence point lookups use the ALREADY SHIPPED (team,seq)
    // indexes. No archive index build or unbounded search by request ID.
    let mut stmt=db.prepare("SELECT request_id,at_ms FROM journal INDEXED BY journal_page WHERE team=?1 AND seq=?2 UNION ALL SELECT request_id,at_ms FROM journal_archive INDEXED BY journal_archive_page WHERE team=?1 AND seq=?2 LIMIT 2").map_err(storage)?;
    let events = stmt
        .query_map(params![team, seq], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)?;
    if events.len() != 1 || events[0].0 != id {
        return Err(invalid("The exact original source event is unavailable."));
    }
    let sizes:Option<(usize,usize)>=db.query_row("SELECT length(CAST(content AS BLOB)),length(CAST(result AS BLOB)) FROM requests WHERE id=?1 AND team=?2",params![id,team],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
    let Some((body_len, result_len)) = sizes else {
        return Err(invalid("The original source receipt is unavailable."));
    };
    if body_len > 16_384 || result_len > 16_384 {
        return Err(over(
            "The original source request exceeds its selected-read bound.",
        ));
    }
    let (raw, result, actor): (String, String, String) = db
        .query_row(
            "SELECT content,result,actor FROM requests WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(storage)?;
    let request: Request = decode(&raw)?;
    let result: serde_json::Value = decode(&result)?;
    let actor: serde_json::Value = decode(&actor)?;
    let revision = result["Success"]["revision"]
        .as_u64()
        .filter(|_| result["Success"]["ok"] == true)
        .ok_or_else(|| invalid("The original source is not a committed success."))?;
    let owner: String = db
        .query_row("SELECT owner FROM teams WHERE id=?1", [team], |r| r.get(0))
        .map_err(storage)?;
    if actor["person"] != owner
        || actor["kind"] != "service"
        || actor["team"] != team
        || request.team != team
        || request.request_id != id
        || request.expected_revision.checked_add(1) != Some(revision)
    {
        return Err(invalid("The original source identity changed."));
    }
    Ok((request, revision, events[0].1))
}
fn event_effect(request: &Request) -> Result<(Option<u32>, String, Category)> {
    Ok(match &request.operation {
        Operation::HandoutIntent {
            node,
            effect_request_id,
            ..
        }
        | Operation::ReopenIntent {
            node,
            effect_request_id,
            ..
        } => (
            Some(*node),
            effect_request_id.clone(),
            Category::DeliveryReserved,
        ),
        Operation::HandoutOutcome {
            effect_request_id,
            outcome,
        }
        | Operation::ReopenOutcome {
            effect_request_id,
            outcome,
        } => (
            None,
            effect_request_id.clone(),
            match outcome {
                HandoutOutcome::Submitted { .. } => Category::DeliverySubmitted,
                HandoutOutcome::Refused { .. } | HandoutOutcome::HandedBack { .. } => {
                    Category::DeliveryRefused
                }
                _ => Category::DeliveryUnknown,
            },
        ),
        _ => {
            return Err(invalid(
                "A timing request cannot observe this operation as a source.",
            ))
        }
    })
}
fn seen(db: &Connection, team: &str, id: &str) -> Result<bool> {
    if !data::present(db)? {
        return Ok(false);
    }
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM observed_timing_facts WHERE team=?1 AND original_request=?2)",
        params![team, id],
        |r| r.get(0),
    )
    .map_err(storage)
}

/// Existing service pass only: one bounded latest LIVE journal window, not an
/// archive drain. An old original outside it stays explicitly unavailable.
/// A selected original retains its real sequence across live-to-archive moves.
pub(crate) fn next_service(
    store: &Store,
    team: &str,
    node: u32,
    epoch: &str,
) -> Result<Option<TimingInput>> {
    store.active()?;
    let Some(db) = store.db.as_ref() else {
        return Ok(None);
    };
    let tx = db.unchecked_transaction().map_err(storage)?;
    let Some(n) = rows::node(db, team, node)? else {
        return Ok(None);
    };
    let Some(task) = n.task else { return Ok(None) };
    let generation:u32=db.query_row("SELECT generation FROM nodes WHERE team=?1 AND id=?2 UNION ALL SELECT generation FROM node_archive WHERE team=?1 AND id=?2 LIMIT 1",params![team,node],|r|r.get(0)).map_err(storage)?;
    let mut stmt=db.prepare("SELECT seq,request_id FROM (SELECT seq,request_id,kind FROM journal INDEXED BY journal_page WHERE team=?1 ORDER BY seq DESC LIMIT 200) WHERE kind IN ('handout_intent','reopen_intent','handout_outcome','reopen_outcome') ORDER BY seq").map_err(storage)?;
    let sources = stmt
        .query_map([team], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)?;
    drop(stmt);
    for (seq, id) in sources {
        if seen(db, team, &id)? {
            continue;
        }
        let (request, _, _) = original(db, team, &id, seq)?;
        let (expected, effect, _) = event_effect(&request)?;
        if expected.is_some_and(|expected| expected != node) {
            continue;
        }
        let owner: Option<(u32, u32)> = db
            .query_row(
                "SELECT node,generation FROM effects WHERE request_id=?1 AND team=?2",
                params![effect, team],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if owner != Some((node, generation)) {
            continue;
        }
        let candidate = TimingInput::Service {
            task,
            node_generation: generation,
            observer_epoch: epoch.into(),
            source_request: id,
            source_sequence: seq,
        };
        tx.commit().map_err(storage)?;
        return Ok(Some(candidate));
    }
    tx.commit().map_err(storage)?;
    Ok(None)
}

fn service(
    ctx: &Context<'_>,
    node: u32,
    task: &TaskRef,
    generation: u32,
    epoch: &str,
    id: &str,
    sequence: u64,
) -> Result<Decision> {
    uuid(id)?;
    if seen(ctx.db, &ctx.team.id, id)? {
        return Ok(Decision::Unchanged);
    }
    let (request, revision, at) = original(ctx.db, &ctx.team.id, id, sequence)?;
    let (expected, effect, category) = event_effect(&request)?;
    let owner: Option<(u32, u32)> = ctx
        .db
        .query_row(
            "SELECT node,generation FROM effects WHERE request_id=?1 AND team=?2",
            params![effect, ctx.team.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if expected.is_some_and(|n| n != node) || owner != Some((node, generation)) {
        return Err(invalid(
            "The original source does not own this exact generation.",
        ));
    }
    let n = ctx.node(node)?;
    // No historical model/publication/member inference from current rows.
    let key = ObservedUnitKey {
        team: ctx.team.id.clone(),
        home: ctx.team.home.clone(),
        node,
        task: task.clone(),
        kind: match n.kind {
            NodeKind::Task => UnitKind::Implementation,
            NodeKind::Check => UnitKind::Check,
            NodeKind::Stage => UnitKind::Stage,
        },
        phase: n.phase,
        publication: None,
        node_generation: generation,
        attempt: None,
        effect: Some(effect),
        binding: None,
    };
    let identity = data::extend(ctx.db)?;
    Ok(Decision::Append(Box::new(TimingFact {
        schema: SCHEMA,
        seq: 0,
        unit: 0,
        key,
        snapshot: HistoricalSnapshot {
            title: String::new(),
            role: None,
            configured_model: None,
            native_model: None,
        },
        source: SourcePosition {
            schema: SCHEMA,
            kind: SourceKind::TeamJournal,
            identity,
            team_revision: revision,
            event_id: Some(id.into()),
            event_sequence: Some(sequence),
            task_revision: None,
            replica_generation: None,
            replica_offset: None,
            observer_epoch: Some(epoch.into()),
        },
        category,
        event_at_ms: Some(at),
        observed_at_ms: ctx.now,
        last_verified_ms: Some(ctx.now),
        break_before: true,
        reasons: vec![
            Reason::NativeUnsupported,
            Reason::OldData,
            Reason::MissingAttempt,
            Reason::MissingBinding,
            Reason::PublicationUnavailable,
        ],
        implementation_observed_ms: None,
    })))
}

/// Read-only coalescing before reserving any request identity. The kernel
/// rechecks the same candidate under its writer transaction.
pub(crate) fn task_needed(
    store: &Store,
    team: &str,
    node: u32,
    sample: &TaskTimingSample,
    now: i64,
) -> Result<bool> {
    let Some(db) = store.db.as_ref() else {
        return Ok(false);
    };
    Ok(!sample::same(db, team, node, sample, now)?)
}
