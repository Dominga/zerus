use super::*;

fn query() -> Query {
    let intent = super::super::test_intent();
    Query {
        request_id: uuid::Uuid::new_v4().to_string(),
        scope: intent.scope,
        reconcile_id: intent.reconcile_id,
        binding: intent.binding,
    }
}
fn submitted(query: &Query) -> Attempt {
    Attempt {
        binding: query.binding.clone(),
        ticket: Ticket {
            operation_id: operation_id(&query.binding.key),
            attempt_id: query.reconcile_id.clone(),
            revision: 1,
            admission_id: uuid::Uuid::new_v4().to_string(),
            binding_sha256: sha256(&serde_json::to_vec(&query.binding).unwrap()),
            writer_epoch: uuid::Uuid::new_v4().to_string(),
            service_epoch: uuid::Uuid::new_v4().to_string(),
            lease: uuid::Uuid::new_v4().to_string(),
        },
        phase: Phase::Submitted,
        outcome: Some(Outcome::Submitted {
            receipt: Receipt {
                schema: 1,
                id: query.binding.key.request_id.clone(),
                project: query.binding.key.project.clone(),
                uuid: query.binding.key.task.clone(),
                state: "submitted".into(),
            },
        }),
        drained: true,
    }
}
fn lookup(query: &Query, attempt: Option<Attempt>) -> Lookup {
    Lookup {
        abi: RESULT_ABI.into(),
        request_id: query.request_id.clone(),
        scope: query.scope.clone(),
        read_only: true,
        found: attempt.is_some(),
        attempt,
    }
}
#[test]
fn result_metadata_needs_both_exact_submission_and_actual_drain() {
    let q = query();
    let mut a = submitted(&q);
    assert_ne!(a.ticket.service_epoch, q.scope.service_epoch);
    assert_ne!(a.ticket.admission_id, q.reconcile_id);
    assert!(a.submitted(&q).is_some());
    a.drained = false;
    assert!(a.matches(&q));
    assert!(a.submitted(&q).is_none());
    a.phase = Phase::Unknown;
    a.outcome = Some(Outcome::Unknown {
        reason: Unknown::ServiceRestart,
    });
    assert!(a.matches(&q));
    assert!(a.submitted(&q).is_none());
    a.drained = true;
    assert!(a.matches(&q));
    assert!(a.submitted(&q).is_none());
    a.phase = Phase::Reserved;
    a.outcome = None;
    assert!(!a.matches(&q));
    a.drained = false;
    assert!(a.matches(&q));
    assert!(a.submitted(&q).is_none());
}
#[test]
fn lookup_absence_is_only_an_exact_read_and_carries_no_attempt() {
    let q = query();
    let reply = lookup(&q, None);
    assert!(reply.matches(&q));
    assert!(reply.attempt.is_none());
    for change in 0..5 {
        let mut bad = reply.clone();
        match change {
            0 => bad.read_only = false,
            1 => bad.found = true,
            2 => bad.request_id = uuid::Uuid::new_v4().to_string(),
            3 => bad.scope.service_epoch = uuid::Uuid::new_v4().to_string(),
            _ => bad.abi = "other".into(),
        }
        assert!(!bad.matches(&q));
    }
}
#[test]
fn changed_ticket_binding_receipt_and_phase_never_settle_another_original() {
    let q = query();
    let original = submitted(&q);
    for change in 0..9 {
        let mut bad = original.clone();
        match change {
            0 => bad.ticket.operation_id = "f".repeat(64),
            1 => bad.ticket.attempt_id = uuid::Uuid::new_v4().to_string(),
            2 => bad.ticket.binding_sha256 = "f".repeat(64),
            3 => bad.ticket.revision = 0,
            4 => bad.binding.executor.run_id = uuid::Uuid::new_v4().to_string(),
            5 => bad.phase = Phase::Unknown,
            n => {
                let Some(Outcome::Submitted { receipt }) = &mut bad.outcome else {
                    unreachable!()
                };
                match n {
                    6 => receipt.id = uuid::Uuid::new_v4().to_string(),
                    7 => receipt.project = "other".into(),
                    _ => receipt.state = "uncertain".into(),
                }
            }
        }
        assert!(!bad.matches(&q), "{change}");
        assert!(bad.submitted(&q).is_none());
    }
}
#[test]
fn query_and_result_are_closed_bounded_data_without_original_text_or_permissions() {
    let q = query();
    let reply = lookup(&q, Some(submitted(&q)));
    assert!(reply.matches(&q));
    let bytes = serde_json::to_vec(&reply).unwrap();
    assert!(bytes.len() < RESULT_BYTES);
    let text = String::from_utf8(bytes).unwrap();
    for field in [
        "raw_record",
        "retained_input",
        "provenance",
        "consent",
        "canonical_reply",
    ] {
        assert!(!text.contains(field));
    }
    let mut value = serde_json::to_value(&q).unwrap();
    value["original"] = serde_json::json!("caller path or body");
    assert!(serde_json::from_value::<Query>(value).is_err());
    let mut value = serde_json::to_value(&reply).unwrap();
    value["permit"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Lookup>(value).is_err());
    for change in 0..5 {
        let mut bad = q.clone();
        match change {
            0 => bad.reconcile_id = bad.binding.key.request_id.clone(),
            1 => bad.request_id = bad.reconcile_id.clone(),
            2 => bad.binding.key.configuration = "f".repeat(64),
            3 => bad.binding.executor.run_id = "x".repeat(513),
            _ => bad.binding.source_record_sha256 = "unknown".into(),
        }
        assert!(!bad.valid());
    }
}
