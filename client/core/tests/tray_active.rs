//! The tray reads the same canonical Active contract as the desktop and mobile lists.
use serde_json::{json, Value};
use std::collections::BTreeMap;
use zerus_client_core::{
    marks::{self, Mark},
    menubar::{Desk, DeviceMarks},
    view, Change, ComputerId, ComputerStatus,
};

#[test]
fn tray_active_matches_the_shared_status_filter_for_local_and_peer_rows() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../mobile/android/app/src/test/resources/session-filter-contract.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        for alias in ["", "peer"] {
            let computer = ComputerId::from(alias);
            let mut raw = case["raw"].clone();
            raw["name"] = json!(format!("codex/contract/{name}"));
            raw["cmd"] = json!("codex");
            let row = view::row(&raw);
            let mark = Mark {
                review: raw["review_later"] == true,
                acknowledged: if raw["attention_acknowledged"] == true {
                    row.attention_fingerprint.clone()
                } else {
                    String::new()
                },
            };
            let key = marks::attention_key(
                alias,
                &row.agent,
                &row.name,
                row.created,
                row.conversation_id.as_deref().unwrap_or_default(),
            );
            let mut desk = Desk::default();
            if case["draft"] == true {
                desk.set_draft_keys(&[format!("{alias}\n{}", row.name)]);
            }
            desk.set_marks(DeviceMarks::new(
                BTreeMap::from([(key, mark)]),
                BTreeMap::new(),
                Vec::new(),
            ));
            desk.observe(&Change::Replace {
                computer: computer.clone(),
                rows: vec![("row".into(), row)],
            });
            desk.observe(&Change::Computer {
                computer,
                status: if case["online"] == true {
                    ComputerStatus::Live
                } else {
                    ComputerStatus::Reconnecting { attempt: 1 }
                },
            });
            let active = case["matches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|filter| filter == "active");
            assert_eq!(desk.active(), usize::from(active), "{name}, {alias:?}");
        }
    }
}

#[test]
fn work_is_current_only_for_live_or_polled_computers() {
    for alias in ["", "peer"] {
        for status in [
            ComputerStatus::Live,
            ComputerStatus::Polled,
            ComputerStatus::Probing,
            ComputerStatus::Unsupported,
            ComputerStatus::Reconnecting { attempt: 1 },
        ] {
            let computer = ComputerId::from(alias);
            let mut desk = Desk::default();
            desk.observe(&Change::Replace {
                computer: computer.clone(),
                rows: vec![(
                    "row".into(),
                    view::row(&json!({"activity":"busy","phase":"tool"})),
                )],
            });
            desk.observe(&Change::Computer { computer, status });
            assert_eq!(
                desk.active(),
                usize::from(matches!(
                    status,
                    ComputerStatus::Live | ComputerStatus::Polled
                )),
                "{alias:?} {status:?}"
            );
        }
    }
}

#[test]
fn draft_identity_and_subagent_keys_match_the_window() {
    let mut desk = Desk::default();
    let id = "00000000-0000-4000-8000-000000000002";
    for alias in ["", "peer"] {
        let computer = ComputerId::from(alias);
        desk.observe(&Change::Replace {
            computer: computer.clone(),
            rows: vec![(
                "row".into(),
                view::row(&json!({"name":"codex/example/one","session_id":id,"activity":"idle"})),
            )],
        });
        desk.observe(&Change::Computer {
            computer,
            status: ComputerStatus::Reconnecting { attempt: 1 },
        });
    }
    for key in [
        format!("peer\nid:{id}"),
        "peer\ncodex/example/one".into(),
        format!("peer\nid:{id}\nconversation\nchild"),
        "peer\ncodex/example/one\nconversation\nchild".into(),
    ] {
        assert!(desk.set_draft_keys(std::slice::from_ref(&key)));
        assert_eq!(desk.active(), 1, "{key:?}: only the exact peer");
        assert!(!desk.set_draft_keys(&[key]));
        assert!(desk.set_draft_keys(&[]));
        assert_eq!(desk.active(), 0);
    }
    desk.set_draft_keys(&["peer\ncodex/example/one\narchive-id".into()]);
    assert_eq!(desk.active(), 0, "an archive's draft is separate");
}
