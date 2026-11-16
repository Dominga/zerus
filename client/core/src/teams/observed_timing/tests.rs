use super::*;
use crate::{
    actions::Exit,
    teams::{
        read::{self, TeamError, TeamRead},
        TeamsFollow, TeamsModel, TeamsUpdate,
    },
    Change, Coalescer, ComputerId,
};
use std::time::Instant;
const TEAM: &str = "00000000-0000-4000-8000-00000000beef";
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../crates/team-plan/fixtures/cli-observed-long.json"
    ))
    .unwrap()
}
fn checked(read: &TeamRead, value: Value) -> Result<Value, TeamError> {
    read::decode(
        read,
        Exit::Code {
            code: 0,
            stdout: serde_json::to_vec(&value).unwrap(),
            stderr: String::new(),
        },
    )
}
fn request(after: Option<String>) -> TeamRead {
    TeamRead::Observed {
        team: TEAM.into(),
        phase: 0,
        node: Some(1),
        window_ms: DEFAULT_WINDOW_MS,
        after,
    }
}
#[test]
fn producer_page_has_only_sampled_or_unavailable_units_and_never_any_actual() {
    let page = decode(&fixture(), TEAM, 0, Some(1), DEFAULT_WINDOW_MS).unwrap();
    assert_eq!(page.cohort.units.len(), 200);
    let pane = project(page, 1, None);
    assert_eq!(pane.coverage.len(), 200);
    assert!(pane
        .coverage
        .iter()
        .all(|u| u.implementation_observed_ms.is_none()));
    assert_eq!(
        pane.coverage
            .iter()
            .filter(|u| u.coverage == CoverageKind::Unavailable)
            .count(),
        199
    );
}
#[test]
fn schema_one_rejects_handwritten_zero_actual_and_every_native_claim() {
    for case in 0..8 {
        let mut value = fixture();
        match case {
            0 => value["observed"]["rows"][0]["implementation_observed_ms"] = 0.into(),
            1 => value["observed"]["rows"][0]["implementation_observed_ms"] = 1.into(),
            2 => value["observed"]["capabilities"]["native_work"] = true.into(),
            3 => value["observed"]["capabilities"]["implementation_work"] = true.into(),
            4 => value["observed"]["rows"][0]["snapshot"]["native_model"] = "invented".into(),
            5 => value["observed"]["rows"][0]["source"]["schema"] = 2.into(),
            6 => value["observed"]["cohort"]["units"][0]["key"]["home"] = "display-alias".into(),
            _ => value["observed"]["rows"][0]["source"]["observer_epoch"] = Value::Null,
        }
        assert!(
            decode(&value, TEAM, 0, Some(1), DEFAULT_WINDOW_MS).is_err(),
            "case{case}"
        );
    }
}
#[test]
fn observed_cursor_has_its_own_bound_without_widening_other_reads() {
    let value = fixture();
    let cursor = value["observed"]["next"].as_str().unwrap().to_owned();
    assert!(cursor.len() > 2048);
    assert!(request(Some(cursor)).args().is_ok());
    for (size, good) in [(8192, true), (8193, false)] {
        assert_eq!(request(Some("x".repeat(size))).args().is_ok(), good);
    }
    assert!(TeamRead::Queue {
        team: TEAM.into(),
        after: Some("x".repeat(2049))
    }
    .args()
    .is_err());
}

// The context rows are written by root's own CLI fixture producer. No rows
// are hand-written here; a selected observation is shown only under that scope.
fn ready() -> (TeamsModel, Instant) {
    let value = fixture();
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            team: Some(TEAM.into()),
            ..Default::default()
        },
        now,
    );
    for op in ["status", "queue"] {
        let read = model.next(now).unwrap();
        model.apply(&read, checked(&read, value["context"][op].clone()), now);
    }
    model.follow_phases(TEAM, Some(0), true, now).unwrap();
    for op in ["phases", "draft"] {
        let read = model.next(now).unwrap();
        model.apply(&read, checked(&read, value["context"][op].clone()), now);
    }
    (model, now)
}
fn assert_absent(updates: Vec<(String, TeamsUpdate)>) {
    let view = updates
        .into_iter()
        .filter_map(|(_, u)| match u {
            TeamsUpdate::Team(v) => Some(v),
            _ => None,
        })
        .next_back()
        .expect("retirement publishes actual frontend state");
    assert!(view.plan.as_ref().unwrap().observed.is_none());
}
#[test]
fn actual_model_batch_drops_page_on_hide_failure_first_and_late_reply() {
    let (mut model, now) = ready();
    let (lease, _) = model
        .follow_observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, true, None, now)
        .unwrap();
    let read = model.next(now).unwrap();
    assert_eq!(read, request(None));
    let updates = model.apply(&read, checked(&read, fixture()), now);
    let mut coalescer = Coalescer::new();
    for (key, update) in updates {
        coalescer.push(Change::teams(ComputerId::local(), key, update));
    }
    let frame = serde_json::to_value(coalescer.take()).unwrap();
    let pane = &frame["teams"][0]["payload"]["plan"]["observed"];
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/teams/fixtures/observed-pane.json");
    if std::env::var_os("ZERUS_UPDATE_OBSERVED_CORE_FIXTURE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(pane).unwrap() + "\n").unwrap();
    } else {
        assert_eq!(
            pane,
            &serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap()
        );
    }
    assert_eq!(
        frame["teams"][0]["payload"]["plan"]["observed"]["page"]["cohort"]["units"]
            .as_array()
            .unwrap()
            .len(),
        200
    );
    assert!(model.page(
        &format!("observed:{TEAM}"),
        fixture()["observed"]["next"].as_str()
    ));
    let late = model.next(now).unwrap();
    assert!(late.after().unwrap().len() > 2048);
    model.set_hidden(true, now);
    assert_absent(model.observed_hidden());
    assert!(model
        .apply(&late, checked(&late, fixture()), now)
        .is_empty());
    model.set_hidden(false, now);
    let fresh = model.next(now).unwrap(); // ordinary reads may refresh first
    let mut next = Some(fresh);
    let observed = loop {
        let r = next.take().unwrap_or_else(|| model.next(now).unwrap());
        if matches!(r, TeamRead::Observed { .. }) {
            break r;
        }
        let key = match r {
            TeamRead::Status { .. } => "status",
            TeamRead::Queue { .. } => "queue",
            TeamRead::Phases { .. } => "phases",
            _ => "draft",
        };
        model.apply(&r, checked(&r, fixture()["context"][key].clone()), now);
    };
    assert!(observed.after().is_none());
    model.apply(&observed, checked(&observed, fixture()), now);
    assert!(model.page(&format!("observed:{TEAM}"), None));
    let observed = model.next(now).unwrap();
    assert_absent(model.apply(
        &observed,
        Err(TeamError::Failed {
            message: "unavailable".into(),
        }),
        now,
    ));
    // Next/First retired reads but did not revoke this component's lease.
    let (_, updates) = model
        .follow_observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, false, Some(lease), now)
        .unwrap();
    assert!(
        updates.is_empty(),
        "already-retired frontend state is coalesced"
    );
    assert!(!model.page(&format!("observed:{TEAM}"), None));
}

#[test]
fn valid_but_wrong_home_and_retired_phase_generation_cannot_publish_coverage() {
    let (mut model, now) = ready();
    let (lease, _) = model
        .follow_observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, true, None, now)
        .unwrap();
    let read = model.next(now).unwrap();
    model.apply(&read, checked(&read, fixture()), now);
    assert!(model.page(&format!("observed:{TEAM}"), None));
    let read = model.next(now).unwrap();
    let mut wrong = fixture();
    let home = "00000000-0000-4000-8000-00000000abcd";
    wrong["observed"]["scope"]["home"] = home.into();
    for unit in wrong["observed"]["cohort"]["units"].as_array_mut().unwrap() {
        unit["key"]["home"] = home.into();
    }
    for row in wrong["observed"]["rows"].as_array_mut().unwrap() {
        row["key"]["home"] = home.into();
    }
    assert!(
        decode(&wrong, TEAM, 0, Some(1), DEFAULT_WINDOW_MS).is_ok(),
        "syntactically valid but wrong source home"
    );
    assert_absent(model.apply(&read, checked(&read, wrong), now));
    assert!(model.page(&format!("observed:{TEAM}"), None));
    let late = model.next(now).unwrap();
    model.follow_phases(TEAM, Some(1), true, now).unwrap();
    assert!(model
        .apply(&late, checked(&late, fixture()), now)
        .is_empty());
    assert!(model
        .follow_observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, false, Some(lease), now)
        .is_none());
}

#[test]
fn hide_then_unhide_retires_a_same_scope_in_flight_read_before_its_reply() {
    let (mut model, now) = ready();
    model
        .follow_observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, true, None, now)
        .unwrap();
    let old = model.next(now).unwrap();
    model.set_hidden(true, now);
    model.observed_hidden();
    model.set_hidden(false, now);
    model.observed_hidden();
    assert!(
        model.apply(&old, checked(&old, fixture()), now).is_empty(),
        "old source reply cannot publish after hide/unhide of the same selection"
    );
}

#[test]
fn existing_single_poller_publishes_retirement_and_observed_long_cursor_through_changes() {
    use crate::teams::TeamsPoller;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;
    let (sent, received) = mpsc::channel();
    let reads = Arc::new(Mutex::new(Vec::new()));
    let seen = reads.clone();
    let poller = TeamsPoller::spawn_with(
        Box::new(move |read| {
            seen.lock().unwrap().push(read.clone());
            let data = fixture();
            let value = match read {
                TeamRead::Observed { .. } => data,
                TeamRead::Status { .. } => data["context"]["status"].clone(),
                TeamRead::Queue { .. } => data["context"]["queue"].clone(),
                TeamRead::Phases { .. } => data["context"]["phases"].clone(),
                TeamRead::Draft { .. } => data["context"]["draft"].clone(),
                _ => panic!("no second reader or unrelated follow is requested"),
            };
            checked(read, value)
        }),
        Box::new(move |change| {
            sent.send(change).unwrap();
        }),
    )
    .unwrap();
    let await_state = |wanted: bool| {
        for _ in 0..20 {
            let change = received.recv_timeout(Duration::from_secs(5)).unwrap();
            if let Change::Teams { payload, .. } = change {
                let plan = &payload["plan"];
                if !plan.is_null() && plan["observed"].is_object() == wanted {
                    return payload;
                }
            }
        }
        panic!("requested observation state was not actually published")
    };
    poller.follow(TeamsFollow {
        team: Some(TEAM.into()),
        ..Default::default()
    });
    poller.phases(TEAM, Some(0), true, None).unwrap();
    let lease = poller
        .observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, true, None)
        .unwrap();
    await_state(true);
    let cursor = fixture()["observed"]["next"].as_str().unwrap().to_owned();
    assert!(cursor.len() > 2048);
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let mut token = URL_SAFE_NO_PAD
        .decode(cursor.strip_prefix("ot1.").unwrap())
        .unwrap();
    token.resize(6141, b' ');
    let cursor = format!("ot1.{}", URL_SAFE_NO_PAD.encode(token));
    assert_eq!(cursor.len(), 8192);
    assert!(!poller.page(&format!("observed:{TEAM}"), Some(&(cursor.clone() + "x"))));
    assert!(poller.page(&format!("observed:{TEAM}"), Some(&cursor)));
    await_state(false);
    await_state(true);
    assert!(reads
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.after() == Some(cursor.as_str())));
    poller.hidden(true);
    await_state(false);
    poller.hidden(false);
    await_state(true);
    assert_eq!(
        poller.observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, false, Some(lease)),
        Some(lease + 1)
    );
    await_state(false);
    drop(poller);
}

#[test]
fn unsupported_older_engine_and_failed_read_retire_without_a_tight_retry_loop() {
    for unavailable in [true, false] {
        let (mut model, now) = ready();
        model
            .follow_observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, true, None, now)
            .unwrap();
        let read = model.next(now).unwrap();
        let error = if unavailable {
            TeamError::Refused {
                code: "not_found".into(),
                message: "old engine".into(),
            }
        } else {
            TeamError::Failed {
                message: "source unreadable".into(),
            }
        };
        model.apply(&read, Err(error), now);
        assert!(
            model.next(now).is_none(),
            "unsupported or failed source cannot spin the single worker"
        );
        if unavailable {
            assert!(model
                .next(now + std::time::Duration::from_secs(1))
                .is_none());
        }
    }
}

#[test]
fn reopening_identical_selection_rejects_old_read_and_old_cleanup_lease() {
    let (mut model, now) = ready();
    let (old_lease, _) = model
        .follow_observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, true, None, now)
        .unwrap();
    let old = model.next(now).unwrap();
    let (new_lease, _) = model
        .follow_observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, true, None, now)
        .unwrap();
    assert!(new_lease > old_lease);
    assert!(
        model.apply(&old, checked(&old, fixture()), now).is_empty(),
        "an identical argument list is not a current read generation"
    );
    let (kept, updates) = model
        .follow_observed(
            TEAM,
            0,
            Some(1),
            DEFAULT_WINDOW_MS,
            false,
            Some(old_lease),
            now,
        )
        .unwrap();
    assert_eq!(kept, new_lease);
    assert!(updates.is_empty());
    let fresh = model.next(now).unwrap();
    assert!(!model
        .apply(&fresh, checked(&fresh, fixture()), now)
        .is_empty());
}

#[test]
fn published_phase_source_reaches_actual_pane_and_batch_without_membership_backfill() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/team-plan/fixtures/cli-observed-phase.json");
    let source: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            team: Some(TEAM.into()),
            ..Default::default()
        },
        now,
    );
    for key in ["status", "queue"] {
        let read = model.next(now).unwrap();
        model.apply(&read, checked(&read, source["context"][key].clone()), now);
    }
    model.follow_phases(TEAM, Some(1), true, now).unwrap();
    for key in ["phases", "draft"] {
        let read = model.next(now).unwrap();
        model.apply(&read, checked(&read, source["context"][key].clone()), now);
    }
    model
        .follow_observed(TEAM, 1, None, DEFAULT_WINDOW_MS, true, None, now)
        .unwrap();
    let read = model.next(now).unwrap();
    let updates = model.apply(&read, checked(&read, source), now);
    let mut coalescer = Coalescer::new();
    for (key, update) in updates {
        coalescer.push(Change::teams(ComputerId::local(), key, update));
    }
    let frame = serde_json::to_value(coalescer.take()).unwrap();
    let view = &frame["teams"][0]["payload"];
    let pane = &view["plan"]["observed"];
    assert_eq!(pane["page"]["scope"]["phase"], 1);
    assert!(pane["page"]["scope"]["publication"].is_object());
    assert!(pane["page"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["key"]["publication"].is_null() && f["implementation_observed_ms"].is_null()));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/teams/fixtures/observed-phase-view.json");
    if std::env::var_os("ZERUS_UPDATE_OBSERVED_PHASE_CORE_FIXTURE").is_some() {
        std::fs::write(path, serde_json::to_string_pretty(view).unwrap() + "\n").unwrap();
    } else {
        assert_eq!(
            view,
            &serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap()
        );
    }
}
#[test]
fn a_recovered_queue_read_cannot_resurrect_pre_failure_observation_coverage() {
    let (mut model, now) = ready();
    model
        .follow_observed(TEAM, 0, Some(1), DEFAULT_WINDOW_MS, true, None, now)
        .unwrap();
    let observed = model.next(now).unwrap();
    model.apply(&observed, checked(&observed, fixture()), now);
    let queue = TeamRead::Queue {
        team: TEAM.into(),
        after: None,
    };
    // A synthetic transport error, after the product-written page reached Core.
    model.apply(
        &queue,
        Err(TeamError::Failed {
            message: "selected read failed".into(),
        }),
        now,
    );
    assert_absent(model.apply(
        &queue,
        checked(&queue, fixture()["context"]["queue"].clone()),
        now,
    ));
}

#[test]
fn admitted_rework_pages_use_the_same_base_and_switch_scope_without_relabelling_facts() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/team-plan/fixtures/cli-observed-rework.json");
    let sources: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            team: Some(TEAM.into()),
            ..Default::default()
        },
        now,
    );
    for key in ["status", "queue"] {
        let read = model.next(now).unwrap();
        model.apply(
            &read,
            checked(&read, sources["reserved"]["context"][key].clone()),
            now,
        );
    }
    let mut views = serde_json::Map::new();
    let mut retired = None;
    for name in ["reserved", "published"] {
        let source = &sources[name];
        let (_, closed) = model.follow_phases(TEAM, Some(1), true, now).unwrap();
        if name == "published" {
            assert_absent(closed);
            let read = retired.take().unwrap();
            assert!(
                model
                    .apply(&read, checked(&read, sources["reserved"].clone()), now)
                    .is_empty(),
                "a pre-switch observed response cannot revive the retained BASE"
            );
        }
        for key in ["phases", "draft"] {
            let read = model.next(now).unwrap();
            model.apply(&read, checked(&read, source["context"][key].clone()), now);
        }
        model
            .follow_observed(TEAM, 1, None, DEFAULT_WINDOW_MS, true, None, now)
            .unwrap();
        let read = model.next(now).unwrap();
        let updates = model.apply(&read, checked(&read, source.clone()), now);
        let mut coalescer = Coalescer::new();
        for (key, update) in updates {
            coalescer.push(Change::teams(ComputerId::local(), key, update));
        }
        let frame = serde_json::to_value(coalescer.take()).unwrap();
        let view = &frame["teams"][0]["payload"];
        let pane = &view["plan"]["observed"];
        assert_eq!(
            pane["page"]["scope"]["publication"]["id"],
            source["context"]["draft"]["draft"]["publication"]["id"]
        );
        assert!(pane["page"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(
                |f| f["key"]["publication"].is_null() && f["implementation_observed_ms"].is_null()
            ));
        views.insert(name.into(), view.clone());
        if name == "reserved" {
            assert!(model.page(&format!("observed:{TEAM}"), None));
            retired = model.next(now);
        }
    }
    let output = Value::Object(views);
    assert_ne!(
        output["reserved"]["plan"]["observed"]["page"]["scope"]["publication"],
        output["published"]["plan"]["observed"]["page"]["scope"]["publication"]
    );
    assert!(model.page(&format!("observed:{TEAM}"), None));
    let fresh = model.next(now).unwrap();
    assert_absent(model.apply(&fresh, checked(&fresh, sources["reserved"].clone()), now));
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/teams/fixtures/observed-rework-views.json");
    if std::env::var_os("ZERUS_UPDATE_OBSERVED_REWORK_CORE_FIXTURE").is_some() {
        std::fs::write(path, serde_json::to_string_pretty(&output).unwrap() + "\n").unwrap();
    } else {
        assert_eq!(
            output,
            serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap()
        );
    }
}
