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
    // The independent producer references committed originals. Canonical
    // delivery operations themselves do not create optional companions.
    let before = h.revision();
    for _ in 0..2 {
        h.apply(
            &service(),
            Operation::RecordTimingFact {
                node,
                observation: Box::new(
                    crate::teams::ops::observed_timing::next_service(
                        &h.store(),
                        TEAM,
                        node,
                        &uid(1702),
                    )
                    .unwrap()
                    .expect("a committed source"),
                ),
            },
        );
    }
    assert_eq!(h.revision(), before + 2);
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

fn source() -> (Harness, u32) {
    let h = Harness::new();
    h.create();
    h.bind("m1", M1_SESSION);
    let node = h.add(1);
    let effect = uid(1801);
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
                receipt: "fixture-source-receipt".into(),
            },
        },
    );
    (h, node)
}
fn timing(h: &Harness, node: u32) -> Operation {
    Operation::RecordTimingFact {
        node,
        observation: Box::new(
            crate::teams::ops::observed_timing::next_service(&h.store(), TEAM, node, &uid(1802))
                .unwrap()
                .expect("a committed source"),
        ),
    }
}

fn optional_tables(store: &crate::teams::store::Store) -> u32 {
    store.db.as_ref().unwrap().query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name LIKE 'observed_timing_%'",[],|r|r.get(0)).unwrap()
}

#[test]
fn optional_recording_storage_fault_does_not_abort_its_canonical_source() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let (h, node) = source();
    let mut store = h.store();
    let revision = h.revision();
    assert_eq!(optional_tables(&store), 0);
    let state = || {
        h.store()
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT state FROM effects WHERE request_id=?1",
                [uid(1801)],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
    };
    assert_eq!(state(), "submitted");
    let request = h.request(timing(&h, node));
    store
        .db
        .as_ref()
        .unwrap()
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::CreateTable { table_name, .. }
                if table_name.starts_with("observed_timing_") =>
            {
                Authorization::Deny
            }
            _ => Authorization::Allow,
        }));
    let error = crate::teams::ops::apply(&mut store, &service(), &request, NOW).unwrap_err();
    assert_eq!(error.code, zerus_team_plan::problem::code::STORAGE_FAILED);
    assert_eq!(h.revision(), revision);
    assert_eq!(state(), "submitted");
    assert_eq!(optional_tables(&h.store()), 0);
    store
        .db
        .as_ref()
        .unwrap()
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    // Read-only recovery first: this settled local transaction has no receipt
    // and is back in autocommit. The same original UUID can enter the kernel.
    assert!(store.db.as_ref().unwrap().is_autocommit());
    assert!(
        crate::teams::ops::replay(store.db.as_ref().unwrap(), &service(), &request)
            .unwrap()
            .is_none()
    );
    crate::teams::ops::apply(&mut store, &service(), &request, NOW).unwrap();
    assert_eq!(state(), "submitted");
    assert_eq!(h.revision(), revision + 1);
    let page = store
        .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_ne!(
        page.rows[0].source.event_id.as_deref(),
        Some(request.request_id.as_str())
    );
    assert_ne!(page.rows[0].source.event_sequence, Some(page.rows[0].seq));
    assert!(page.rows[0].source.team_revision < page.source_revision);
}

#[test]
fn service_only_replay_stale_and_exhausted_source_do_not_feed_the_ledger() {
    let (h, node) = source();
    let operation = timing(&h, node);
    let mut store = h.store();
    let denied = h.request(operation.clone());
    assert_eq!(
        crate::teams::ops::apply(&mut store, &person(), &denied, NOW)
            .unwrap_err()
            .code,
        zerus_team_plan::problem::code::NOT_ALLOWED
    );
    assert_eq!(optional_tables(&store), 0);
    let mut stale = h.request(operation.clone());
    stale.expected_revision = 0;
    assert_eq!(
        crate::teams::ops::apply(&mut store, &service(), &stale, NOW)
            .unwrap_err()
            .code,
        zerus_team_plan::problem::code::STALE_REVISION
    );
    assert_eq!(optional_tables(&store), 0);
    let request = h.request(operation.clone());
    let receipt = crate::teams::ops::apply(&mut store, &service(), &request, NOW).unwrap();
    assert_eq!(
        crate::teams::ops::apply(&mut store, &service(), &request, NOW + 1).unwrap(),
        receipt
    );
    h.apply(&service(), timing(&h, node));
    let revision = h.revision();
    let journal = h.store().journal(TEAM, 200, None).unwrap();
    for _ in 0..3 {
        assert_eq!(h.apply(&service(), operation.clone()).revision, revision);
    }
    assert_eq!(h.revision(), revision);
    assert_eq!(h.store().journal(TEAM, 200, None).unwrap(), journal);
    let page = h
        .store()
        .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    assert!(page
        .rows
        .iter()
        .all(|f| f.source.team_revision < page.source_revision));
}

fn refused_unit(h: &Harness, node: u32, n: u32) {
    let effect = uid(20_000 + n);
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
            outcome: HandoutOutcome::Refused {
                reason: "Fixture proves no native input".into(),
            },
        },
    );
    for _ in 0..2 {
        h.apply(&service(), timing(h, node));
    }
}

#[test]
fn two_hundred_exact_units_of_one_node_have_a_frozen_long_cursor() {
    use zerus_team_plan::observed_timing::{Reason, MAX_CURSOR_BYTES};
    let _fixture_stream = crate::teams::store::observed_timing::fixture_stream();
    let h = Harness::new();
    h.create();
    h.bind("m1", M1_SESSION);
    let node = h.add(1);
    for n in 0..200 {
        refused_unit(&h, node, n);
    }
    let page = h
        .store()
        .observed_timing(TEAM, 0, Some(node), 86_400_000, 1, None)
        .unwrap();
    assert_eq!(page.cohort.units.len(), 200);
    assert!(!page.cohort.more);
    assert!(page.cohort.units.iter().all(|u| u.key.node == node));
    let unique: std::collections::BTreeSet<_> = page
        .cohort
        .units
        .iter()
        .map(|u| u.key.effect.clone())
        .collect();
    assert_eq!(unique.len(), 200);
    let cursor = page.next.clone().expect("one-fact page has more");
    assert!(cursor.len() > 2048, "{}", cursor.len());
    assert!(cursor.len() <= MAX_CURSOR_BYTES);
    let mut fixture =
        serde_json::json!({"ok":true,"actor":{"kind":"person"},"team":TEAM,"observed":page});
    for (key, command) in [
        ("status", format!("status --team {TEAM}")),
        ("queue", format!("queue --team {TEAM}")),
        ("phases", format!("phases --team {TEAM}")),
        ("draft", format!("phase --team {TEAM} --phase 0")),
    ] {
        let (value, exit) = h.cli(&Caller::Person, &command, "");
        assert_eq!(exit, 0, "{value}");
        fixture["context"][key] = value;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/team-plan/fixtures/cli-observed-long.json");
    if std::env::var_os("ZERUS_UPDATE_OBSERVED_FIXTURE").is_some() {
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&fixture).unwrap() + "\n",
        )
        .unwrap();
    } else {
        assert_eq!(
            fixture,
            serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(path).unwrap())
                .unwrap()
        );
    }
    let before = h
        .store()
        .observed_timing(TEAM, 0, Some(node), 86_400_000, 1, Some(&cursor))
        .unwrap();
    refused_unit(&h, node, 200);
    let after = h
        .store()
        .observed_timing(TEAM, 0, Some(node), 86_400_000, 1, Some(&cursor))
        .unwrap();
    assert_eq!(after, before, "new keys/events cannot change a frozen page");
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let mut raw = URL_SAFE_NO_PAD
        .decode(cursor.strip_prefix("ot1.").unwrap())
        .unwrap();
    raw.resize(6141, b' ');
    let padded = format!("ot1.{}", URL_SAFE_NO_PAD.encode(raw));
    assert_eq!(padded.len(), 8192);
    assert_eq!(
        h.store()
            .observed_timing(TEAM, 0, Some(node), 86_400_000, 1, Some(&padded))
            .unwrap(),
        before
    );
    assert!(h
        .store()
        .observed_timing(TEAM, 0, Some(node), 86_400_000, 1, Some(&(padded + "x")))
        .is_err());
    let fresh = h
        .store()
        .observed_timing(TEAM, 0, Some(node), 86_400_000, 200, None)
        .unwrap();
    assert_eq!(fresh.cohort.units.len(), 200);
    assert!(fresh.cohort.more && fresh.truncated);
    assert!(fresh.reasons.contains(&Reason::CohortCap));
    assert_eq!(
        fresh.fact_ceiling, page.fact_ceiling,
        "off-cohort later source cannot advance the ceiling"
    );
    assert_eq!(fresh.as_of_ms, page.as_of_ms);
    h.apply(&person(), Operation::Stop);
    h.apply(&person(), Operation::Archive);
    assert_eq!(
        h.store()
            .observed_timing(TEAM, 0, Some(node), 86_400_000, 1, Some(&cursor))
            .unwrap(),
        before,
        "archive cannot mutate a retained immutable sample"
    );
    assert!(h
        .store()
        .observed_timing(TEAM, 1, Some(node), 86_400_000, 1, Some(&cursor))
        .is_err());
    assert!(h
        .store()
        .observed_timing(TEAM, 0, None, 86_400_000, 1, Some(&cursor))
        .is_err());
    assert!(h
        .store()
        .observed_timing(TEAM, 0, Some(node), 1, 1, Some(&cursor))
        .is_err());
    let too_long = "x".repeat(MAX_CURSOR_BYTES + 1);
    assert!(h
        .store()
        .observed_timing(TEAM, 0, Some(node), 86_400_000, 1, Some(&too_long))
        .is_err());
}

#[test]
fn observation_read_without_a_valid_source_has_no_invented_clock_or_tables() {
    let h = Harness::new();
    h.create();
    let before = h.revision();
    let page = h
        .store()
        .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
        .unwrap();
    assert!(page.as_of_ms.is_none() && page.last_verified_ms.is_none());
    assert!(page.scope.window.from_ms.is_none() && page.scope.window.to_ms.is_none());
    assert!(page.rows.is_empty() && page.cohort.units.is_empty());
    assert_eq!(optional_tables(&h.store()), 0);
    assert_eq!(h.revision(), before);
}

#[test]
fn optional_initialization_uses_no_archive_index_and_settles_a_competing_canonical_writer() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};
    let (h, node) = source();
    let selected = timing(&h, node);
    h.create_team(TEAM_B, "Other fixture", "zerus/team/other-fixture");
    // Every retained archive row is produced by the canonical journal writer.
    for n in 0..10_010 {
        h.apply(
            &person(),
            Operation::RenameTeam {
                name: format!("Fixture {n}"),
            },
        );
    }
    let mut store = h.store();
    let db = store.db.as_ref().unwrap();
    let archive: u32 = db
        .query_row("SELECT count(*) FROM journal_archive", [], |r| r.get(0))
        .unwrap();
    assert!(archive > 0);
    assert!(
        crate::teams::ops::observed_timing::next_service(&store, TEAM, node, &uid(1970))
            .unwrap()
            .is_none(),
        "old sources outside the selected live window are unavailable, never drained"
    );
    let indexes:Vec<String>=db.prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name IN ('journal','journal_archive','requests','effects') ORDER BY name").unwrap().query_map([],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    for (sql, expected) in [
        (
            "EXPLAIN QUERY PLAN SELECT request_id FROM journal INDEXED BY journal_page WHERE team=?1 AND seq=?2",
            "journal_page",
        ),
        (
            "EXPLAIN QUERY PLAN SELECT request_id FROM journal_archive INDEXED BY journal_archive_page WHERE team=?1 AND seq=?2",
            "journal_archive_page",
        ),
    ] {
        let plan: String = db
            .prepare(sql)
            .unwrap()
            .query_map(rusqlite::params![TEAM, 1], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");
        assert!(plan.contains(expected), "{plan}");
    }
    let selected = h.request(selected);
    let other = crate::teams::ops::Request {
        request_id: uid(1_000_000),
        team: TEAM_B.into(),
        expected_revision: h.revision_of(TEAM_B),
        operation: Operation::Pause,
    };
    let state = h.state().to_path_buf();
    let ready = Arc::new(Barrier::new(2));
    let go = ready.clone();
    let once = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let entered = once.clone();
    store.db.as_ref().unwrap().authorizer(Some(move |c:AuthContext<'_>| {
        if matches!(c.action,AuthAction::CreateTable{table_name,..} if table_name=="observed_timing_stream") && !entered.swap(true,std::sync::atomic::Ordering::SeqCst) {
            go.wait();std::thread::sleep(Duration::from_millis(100));
        }
        Authorization::Allow
    }));
    let canonical = std::thread::spawn(move || {
        ready.wait();
        let start = Instant::now();
        let mut s = crate::teams::store::Store::open(&state).unwrap();
        let r = crate::teams::ops::apply(&mut s, &person(), &other, NOW);
        (r, start.elapsed())
    });
    let start = Instant::now();
    crate::teams::ops::apply(&mut store, &service(), &selected, NOW).unwrap();
    let elapsed = start.elapsed();
    let (result, wait) = canonical.join().unwrap();
    result.unwrap();
    assert!(
        elapsed < Duration::from_secs(3) && wait < Duration::from_secs(3),
        "optional={elapsed:?} canonical={wait:?}"
    );
    eprintln!("retained archive={archive}; optional initialization={elapsed:?}; competing canonical={wait:?}");
    let after:Vec<String>=store.db.as_ref().unwrap().prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name IN ('journal','journal_archive','requests','effects') ORDER BY name").unwrap().query_map([],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    assert_eq!(indexes, after);
    assert_eq!(
        store
            .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
            .unwrap()
            .rows
            .len(),
        1
    );
}

#[test]
fn closed_producer_rejects_handwritten_schema_one_duration_and_native_claims() {
    let (h, node) = source();
    h.apply(&service(), timing(&h, node));
    let page = h
        .store()
        .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
        .unwrap();
    let original = page.rows[0].clone();
    for case in 0..4 {
        let mut row = original.clone();
        row.seq += 100;
        match case {
            0 => row.implementation_observed_ms = Some(0),
            1 => row.implementation_observed_ms = Some(1),
            2 => row.snapshot.native_model = Some("invented model".into()),
            _ => row.source.schema = 2,
        }
        assert!(
            crate::teams::store::observed_timing::append(h.store().db.as_ref().unwrap(), &row)
                .is_err(),
            "case{case}"
        );
    }
    assert_eq!(
        h.store()
            .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
            .unwrap(),
        page
    );
}

#[test]
#[ignore = "private current -> admitted older engine -> current storage qualification"]
fn current_writer_exports_an_owned_companion_compatibility_fixture() {
    let destination = std::path::PathBuf::from(
        std::env::var_os("ZERUS_TIMING_COMPAT_STATE").expect("owned fixture destination"),
    );
    assert!(destination.join(".observed-compat-fixture").is_file());
    assert!(destination.canonicalize().unwrap().starts_with(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .canonicalize()
            .unwrap()
    ));
    let (h, node) = source();
    h.apply(&service(), timing(&h, node));
    let store = h.store();
    let db = store.db.as_ref().unwrap();
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    let tables: Vec<(String, String)> = db
        .prepare("SELECT name,sql FROM sqlite_master WHERE name LIKE 'observed_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let page = store
        .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
        .unwrap();
    let layout: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(layout, 4);
    let teams = destination.join("teams");
    crate::platform::private_dir(&teams).unwrap();
    assert!(!teams.join("teams-v1.sqlite3").exists());
    std::fs::copy(
        h.state().join("teams/teams-v1.sqlite3"),
        teams.join("teams-v1.sqlite3"),
    )
    .unwrap();
    std::fs::write(teams.join("preview"), "on\n").unwrap();
    std::fs::write(
        destination.join("before.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"page":page,"layout":layout,"tables":tables}),
        )
        .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "private current -> admitted older engine -> current storage qualification"]
fn current_writer_reopens_after_older_canonical_write_without_changing_companion_history() {
    let destination = std::path::PathBuf::from(
        std::env::var_os("ZERUS_TIMING_COMPAT_STATE").expect("owned fixture destination"),
    );
    assert!(destination.join(".observed-compat-fixture").is_file());
    let before: serde_json::Value =
        serde_json::from_slice(&std::fs::read(destination.join("before.json")).unwrap()).unwrap();
    let mut store = crate::teams::store::Store::open(&destination).unwrap();
    let team = store.team(TEAM).unwrap().unwrap();
    assert_eq!(team.status, zerus_team_plan::TeamStatus::Paused);
    let page = store
        .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&page.rows).unwrap(),
        before["page"]["rows"]
    );
    let node = page.rows[0].key.node;
    let candidate =
        crate::teams::ops::observed_timing::next_service(&store, TEAM, node, &uid(1980))
            .unwrap()
            .unwrap();
    let request = crate::teams::ops::Request {
        request_id: uid(1981),
        team: TEAM.into(),
        expected_revision: team.revision,
        operation: Operation::RecordTimingFact {
            node,
            observation: Box::new(candidate),
        },
    };
    crate::teams::ops::apply(&mut store, &service(), &request, NOW + 1).unwrap();
    assert_eq!(
        store
            .observed_timing(TEAM, 0, None, 86_400_000, 200, None)
            .unwrap()
            .rows
            .len(),
        2
    );
    let layout: u32 = store
        .db
        .as_ref()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(layout, 4);
}
