//! Service facts are made by the existing product writers, never SQL rows.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::{HandoutOutcome, Operation};

#[test]
fn service_writer_facts_are_observed_without_claiming_implementation_time() {
    let h = Harness::new();
    h.create();
    h.bind("m1", M1_SESSION);
    let node = h.add(1);
    let effect = uid(1701);
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node,
            member: "m1".into(),
            effect_request_id: effect.clone(),
        },
    );
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: effect,
            outcome: HandoutOutcome::Submitted {
                receipt: "fixture-recorded-outcome".into(),
            },
        },
    );
    let (reply, exit) = h.cli(
        &Caller::Person,
        &format!("observed --team {TEAM} --phase 0 --window-ms 86400000"),
        "",
    );
    assert_eq!(exit, 0, "{reply}");
    let page = &reply["observed"];
    let rows = page["rows"].as_array().expect("typed observation rows");
    assert!(rows.iter().any(|r| r["category"] == "delivery_reserved"));
    assert!(rows.iter().any(|r| r["category"] == "delivery_submitted"));
    assert_eq!(page["capabilities"]["implementation_work"], false);
    assert!(rows
        .iter()
        .all(|r| r["implementation_observed_ms"].is_null()));
}
