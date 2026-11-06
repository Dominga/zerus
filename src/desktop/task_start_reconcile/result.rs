//! Observe one original attempt under one read snapshot. No execution path.
use super::*;

pub(in crate::desktop) fn query(store: &Store, value: &Value) -> Value {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        op: String,
        query: wire::Query,
    }
    let read = (|| -> Result<wire::Lookup> {
        let read: Read = serde_json::from_value(value.clone())?;
        ensure!(
            read.op == "task_start_reconcile_result" && read.query.valid(),
            "invalid task result query"
        );
        let query = read.query;
        let expected = operations::to_ledger(&query.binding);
        let operation_id = expected.key.operation_id()?;
        // The scope, ticket, frozen binding and outcome are one SQLite read
        // snapshot, including when another process revokes or restarts.
        let tx = store.db.unchecked_transaction()?;
        let current: (String, String) = tx.query_row(
            "SELECT configuration,epoch FROM service WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            current
                == (
                    query.scope.configuration.clone(),
                    query.scope.service_epoch.clone()
                ),
            "task result scope changed"
        );
        let row: Option<(String, String, String, Option<String>, i64)> = tx.query_row(
            "SELECT substr(ticket,1,4097),substr(fingerprint,1,65),substr(state,1,17),substr(outcome_json,1,16385),drained FROM desktop_operation_attempts WHERE attempt_id=?1",
            [&query.reconcile_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        let attempt = row
            .map(
                |(encoded, fingerprint, state, outcome, drained)| -> Result<wire::Attempt> {
                    ensure!(
                        encoded.len() <= 4096
                            && fingerprint.len() == 64
                            && outcome
                                .as_ref()
                                .is_none_or(|v| v.len() <= zerus_desktop_ledger::RECEIPT_BYTES)
                            && matches!(drained, 0 | 1),
                        "task result metadata unavailable"
                    );
                    let ticket: Ticket = serde_json::from_str(&encoded)?;
                    ensure!(
                        ticket.fingerprint()? == fingerprint && ticket.operation_id == operation_id,
                        "task result ticket changed"
                    );
                    let frozen = ledger::load(&tx, &operation_id)?;
                    ensure!(
                        frozen.binding == expected
                            && ticket.admission_id == frozen.admitted.admission_id
                            && ticket.revision == frozen.admitted.revision
                            && ticket.binding_sha256 == frozen.admitted.binding_sha256,
                        "task result original changed"
                    );
                    operations::VerifiedOriginal::verify(
                        query.binding.clone(),
                        &frozen.raw_record,
                        &frozen.retained_input,
                        &frozen.provenance,
                    )
                    .map_err(|_| anyhow::anyhow!("task result frozen original unavailable"))?;
                    let phase = match state.as_str() {
                        "reserved" => wire::Phase::Reserved,
                        "submitted" => wire::Phase::Submitted,
                        "review" => wire::Phase::Review,
                        "unknown" => wire::Phase::Unknown,
                        _ => anyhow::bail!("task result phase unavailable"),
                    };
                    let outcome: Option<ledger::Outcome> =
                        outcome.map(|v| serde_json::from_str(&v)).transpose()?;
                    let outcome = outcome.map(|v| match v {
                        ledger::Outcome::Submitted { receipt } => wire::Outcome::Submitted {
                            receipt: wire::Receipt {
                                schema: receipt.schema,
                                id: receipt.id,
                                project: receipt.project,
                                uuid: receipt.uuid,
                                state: receipt.state,
                            },
                        },
                        ledger::Outcome::Review { reason } => wire::Outcome::Review {
                            reason: match reason {
                                ledger::Review::Policy => wire::Review::Policy,
                                ledger::Review::BeforeSpawnRefused => {
                                    wire::Review::BeforeSpawnRefused
                                }
                                ledger::Review::InvalidReply => wire::Review::InvalidReply,
                                ledger::Review::Cancelled => wire::Review::Cancelled,
                            },
                        },
                        ledger::Outcome::Unknown { reason } => wire::Outcome::Unknown {
                            reason: match reason {
                                ledger::Unknown::LostResult => wire::Unknown::LostResult,
                                ledger::Unknown::TimedOut => wire::Unknown::TimedOut,
                                ledger::Unknown::Cancelled => wire::Unknown::Cancelled,
                                ledger::Unknown::ServiceRestart => wire::Unknown::ServiceRestart,
                            },
                        },
                    });
                    let Ticket {
                        operation_id,
                        attempt_id,
                        revision,
                        admission_id,
                        binding_sha256,
                        writer_epoch,
                        service_epoch,
                        lease,
                    } = ticket;
                    Ok(wire::Attempt {
                        binding: query.binding.clone(),
                        ticket: wire::Ticket {
                            operation_id,
                            attempt_id,
                            revision,
                            admission_id,
                            binding_sha256,
                            writer_epoch,
                            service_epoch,
                            lease,
                        },
                        phase,
                        outcome,
                        drained: drained == 1,
                    })
                },
            )
            .transpose()?;
        let reply = wire::Lookup {
            abi: wire::RESULT_ABI.into(),
            request_id: query.request_id.clone(),
            scope: query.scope.clone(),
            read_only: true,
            found: attempt.is_some(),
            attempt,
        };
        ensure!(reply.matches(&query), "task result binding unavailable");
        tx.commit()?;
        Ok(reply)
    })();
    read.map(|v| json!(v)).unwrap_or_else(
        |_| json!({"accepted":false,"read_only":true,"code":"task_start_result_unavailable"}),
    )
}
