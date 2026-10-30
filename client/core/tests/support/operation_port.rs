//! Test-only conformance oracle for G1 fake-port observations. This is NOT an
//! actor/ledger/runner implementation. G1 may adapt its actual test-port trace
//! into these events to reuse the independently specified vectors and checks.
use serde::Deserialize;
use std::collections::BTreeMap;
use zerus_client_core::operation_recovery::task_start::{Binding, Original, Review};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub abi: String,
    pub model_only: bool,
    pub raw_record: String,
    pub retained_input: String,
    pub binding: Binding,
    pub expected_operation_id: String,
    pub expected_plan_sha256: String,
    pub expected_consent_sha256: String,
    pub tickets: BTreeMap<String, Ticket>,
    pub cases: Vec<Case>,
}
impl Fixture {
    pub fn original(&self, input_available: bool) -> Result<Original, Review> {
        Original::verify(
            self.binding.clone(),
            self.raw_record.as_bytes(),
            if input_available {
                self.retained_input.as_bytes()
            } else {
                b""
            },
        )
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    pub operation_id: String,
    pub attempt_id: String,
    pub revision: u64,
    pub admission_id: String,
    pub binding_sha256: String,
    pub writer_epoch: String,
    pub service_epoch: String,
    pub lease: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub input_available: bool,
    #[serde(default)]
    pub unsettled: usize,
    pub trace: Vec<Event>,
    pub expected_error: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub t: String,
    pub ticket: Option<String>,
    pub fresh: Option<bool>,
    pub input_sha256: Option<String>,
    pub outcome: Option<String>,
    pub reason: Option<String>,
    pub receipt_id: Option<String>,
}

#[derive(Default)]
struct Observed {
    fresh: bool,
    spawned: bool,
    outcome: Option<String>,
    outcome_detail: Option<(String, Option<String>, Option<String>)>,
    child: bool,
    stdout: bool,
    stderr: bool,
    drained: bool,
}

/// Checks observations, never calls a port or creates any operation. Incomplete
/// traces may retain admission: absence of a release is a valid unknown state.
pub fn check_trace(fixture: &Fixture, case: &Case) -> Result<(), &'static str> {
    if case.trace.len() > 64 {
        return Err("trace_bound");
    }
    let mut guard = false;
    let mut admitted = false;
    let mut explicit = false;
    let mut active = None;
    let mut status_ticket = None;
    let mut reservations: BTreeMap<String, (Ticket, Observed)> = BTreeMap::new();
    for event in &case.trace {
        if !case.input_available && event.t != "review" {
            return Err("missing_retained_input");
        }
        let ticket = event
            .ticket
            .as_ref()
            .map(|name| fixture.tickets.get(name).ok_or("unknown_ticket"))
            .transpose()?;
        match event.t.as_str() {
            "guard_current" => guard = true,
            "review" => {
                if event.reason.as_deref() != Some("missing_retained_input") {
                    return Err("review_reason_required");
                }
            }
            "admission_refused" | "reserve_refused" => {
                if event.reason.is_none() {
                    return Err("refusal_reason_required");
                }
            }
            "admitted" => {
                if case.unsettled >= 32 {
                    return Err("admission_pressure");
                }
                if !guard {
                    return Err("current_guard_required");
                }
                admitted = true;
            }
            "revoke" | "restart" => guard = false,
            "explicit_lookup" => explicit = true,
            "reserved" => {
                if !guard || !admitted {
                    return Err("current_guard_required");
                }
                let ticket = ticket.ok_or("exact_ticket_required")?;
                if ticket.operation_id != fixture.expected_operation_id {
                    return Err("original_operation_required");
                }
                let fresh = event.fresh.ok_or("fresh_flag_required")?;
                if !fresh {
                    status_ticket = Some(ticket.clone());
                }
                if fresh {
                    if active.is_some() {
                        return Err("one_undrained_attempt");
                    }
                    if !explicit {
                        return Err("explicit_lookup_required");
                    }
                    if reservations.contains_key(&ticket.attempt_id) {
                        return Err("attempt_identity_immutable");
                    }
                    active = Some(ticket.attempt_id.clone());
                    explicit = false;
                }
                if reservations
                    .get(&ticket.attempt_id)
                    .is_some_and(|(reserved, _)| reserved != ticket)
                {
                    return Err("exact_ticket_required");
                }
                reservations
                    .entry(ticket.attempt_id.clone())
                    .or_insert_with(|| {
                        (
                            ticket.clone(),
                            Observed {
                                fresh,
                                ..Observed::default()
                            },
                        )
                    });
            }
            "status_only" => {
                let ticket = ticket.ok_or("exact_ticket_required")?;
                if status_ticket.as_ref() != Some(ticket) {
                    return Err("status_requires_existing_reservation");
                }
            }
            _ => {
                let ticket = ticket.ok_or("exact_ticket_required")?;
                let Some((reserved, seen)) = reservations.get_mut(&ticket.attempt_id) else {
                    return Err(if event.t == "spawn" {
                        "reserve_before_spawn"
                    } else {
                        "exact_ticket_required"
                    });
                };
                if reserved != ticket {
                    return Err("exact_ticket_required");
                }
                if !seen.fresh {
                    return Err("fresh_reservation_required");
                }
                match event.t.as_str() {
                    "spawn" => {
                        if !guard {
                            return Err("current_guard_required");
                        }
                        if seen.spawned || seen.outcome.is_some() {
                            return Err("one_spawn_per_attempt");
                        }
                        if event.input_sha256.as_deref() != Some(&fixture.expected_plan_sha256) {
                            return Err("original_input_required");
                        }
                        seen.spawned = true;
                    }
                    "finish" | "recover_unknown" => {
                        let outcome = if event.t == "recover_unknown" {
                            "unknown"
                        } else {
                            event.outcome.as_deref().ok_or("outcome_required")?
                        };
                        if !matches!(outcome, "submitted" | "review" | "unknown") {
                            return Err("invalid_outcome");
                        }
                        if outcome == "submitted"
                            && event.receipt_id.as_deref() != Some(&fixture.binding.key.request_id)
                        {
                            return Err("original_receipt_required");
                        }
                        let detail = (
                            outcome.to_owned(),
                            event.receipt_id.clone(),
                            event.reason.clone(),
                        );
                        if seen
                            .outcome_detail
                            .as_ref()
                            .is_some_and(|old| old != &detail)
                        {
                            return Err("terminal_outcome_immutable");
                        }
                        seen.outcome = Some(outcome.into());
                        seen.outcome_detail = Some(detail);
                    }
                    "finish_rejected" => {
                        if seen.outcome.as_deref() != Some("unknown")
                            || event.outcome.as_deref() != Some("submitted")
                        {
                            return Err("unknown_result_required");
                        }
                    }
                    "stop_returned" => (), // never establishes actual drain
                    "child_done" => seen.child = true,
                    "stdout_done" => seen.stdout = true,
                    "stderr_done" => seen.stderr = true,
                    "drained" => {
                        if !(seen.child && seen.stdout && seen.stderr) {
                            return Err("actual_drain_required");
                        }
                        if seen.outcome.is_none() {
                            return Err("outcome_required");
                        }
                        seen.drained = true;
                    }
                    "release" => {
                        if !seen.drained {
                            return Err("verified_drain_before_release");
                        }
                        if active.as_ref() != Some(&ticket.attempt_id) {
                            return Err("exact_ticket_required");
                        }
                        active = None;
                    }
                    _ => return Err("unknown_event"),
                }
            }
        }
    }
    Ok(())
}
