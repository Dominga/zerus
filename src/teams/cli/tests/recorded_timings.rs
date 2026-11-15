//! Timing rows are produced by the current task/stage operation writers.
use super::harness::*;
use crate::teams::cli::actor::Caller;

#[test]
fn recorded_timing_read_reuses_the_journal_page_without_writing_or_claiming_work() {
    let (h, stage) = super::verdict::stage_harness();
    let revision = h.revision();
    let (plain, exit) = h.cli(&Caller::Person, &format!("journal --team {TEAM}"), "");
    assert_eq!(exit, 0, "{plain}");
    let (reply, exit) = h.cli(
        &Caller::Person,
        &format!("journal --team {TEAM} --recorded-timings"),
        "",
    );
    assert_eq!(exit, 0, "{reply}");
    let pinned: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../crates/team-plan/fixtures/cli-journal-timings.json"
    ))
    .unwrap();
    assert_eq!(
        reply, pinned,
        "the timing fixture comes from the product read and operation writers"
    );
    assert_eq!(reply["timings"]["schema"], 1);
    assert_eq!(reply["timings"]["revision"], revision);
    assert_eq!(
        reply["rows"].as_array().unwrap().len(),
        plain["rows"].as_array().unwrap().len()
    );
    let rows = reply["rows"].as_array().unwrap();
    assert!(rows
        .iter()
        .any(|row| row["timing"]["effect"]["first_outcome_recorded_ms"].is_i64()));
    assert!(rows
        .iter()
        .any(|row| row["timing"]["stage"]["node"] == stage));
    assert_eq!(h.revision(), revision, "a timing read wrote team state");
    assert!(!reply.to_string().contains("worked_ms"));
}

#[test]
fn timing_flag_keeps_person_and_exact_lead_authority_and_existing_page_bounds() {
    let h = seeded();
    for caller in [Caller::Person, Caller::Session(key(LEAD_SESSION))] {
        let (first, code) = h.cli(
            &caller,
            &format!("journal --team {TEAM} --recorded-timings --limit 1"),
            "",
        );
        assert_eq!(code, 0, "{first}");
        assert_eq!(first["rows"].as_array().unwrap().len(), 1);
        let token = first["next"].as_str().unwrap();
        let (next, code) = h.cli(
            &caller,
            &format!("journal --team {TEAM} --recorded-timings --limit 1 --after {token}"),
            "",
        );
        assert_eq!(code, 0, "{next}");
        assert!(next["rows"][0]["seq"].as_u64() > first["rows"][0]["seq"].as_u64());
    }
    let (_, code) = h.cli(
        &Caller::Session(key(M1_SESSION)),
        &format!("journal --team {TEAM} --recorded-timings"),
        "",
    );
    assert_eq!(code, 1);
    for line in [
        format!("journal --team {TEAM} --recorded-timings --limit 201"),
        format!("inbox --team {TEAM} --recorded-timings"),
    ] {
        assert_eq!(h.cli(&Caller::Person, &line, "").1, 1);
    }
}

#[test]
fn selected_event_ids_exclude_new_events_and_survive_the_real_archive_writer() {
    let h = seeded();
    let (first, status) = h.cli(
        &Caller::Person,
        &format!("journal --team {TEAM} --limit 2"),
        "",
    );
    assert_eq!(status, 0);
    let ids = first["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["seq"].as_u64().unwrap().to_string())
        .collect::<Vec<_>>()
        .join(",");
    h.apply(
        &person(),
        crate::teams::ops::Operation::OpenCard {
            kind: "note".into(),
            text: "Added later".into(),
            node: None,
            decisions: None,
            parameters: None,
        },
    );
    for archive in [false, true] {
        if archive {
            h.apply(&person(), crate::teams::ops::Operation::Stop);
            h.apply(&person(), crate::teams::ops::Operation::Archive);
        }
        let (joined, status) = h.cli(
            &Caller::Person,
            &format!("journal --team {TEAM} --recorded-timings --timing-seqs {ids}"),
            "",
        );
        assert_eq!(status, 0, "{joined}");
        assert_eq!(
            joined["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["seq"].clone())
                .collect::<Vec<_>>(),
            first["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["seq"].clone())
                .collect::<Vec<_>>()
        );
    }
    for ids in ["1,1", "2,1", "0", "9007199254740992", "9999999", "01"] {
        assert_eq!(
            h.cli(
                &Caller::Person,
                &format!("journal --team {TEAM} --recorded-timings --timing-seqs {ids}"),
                ""
            )
            .1,
            1,
            "{ids}"
        );
    }
}

#[test]
fn selected_empty_page_is_explicit_and_never_becomes_a_fresh_history_read() {
    let h = seeded();
    let words = [
        "journal",
        "--team",
        TEAM,
        "--recorded-timings",
        "--timing-seqs",
        "none",
    ]
    .map(str::to_owned);
    let (reply, status) = super::super::run(&h.config, &Caller::Person, &words, Some(""));
    assert_eq!(status, 0, "{reply}");
    assert_eq!(reply["rows"], serde_json::json!([]));
}
