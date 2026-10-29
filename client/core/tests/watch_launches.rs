//! Additive launch binding fields; Qt reads these in HgsClient.cpp:184,279.
use serde_json::{json, Value};
use zerus_client_core::{machine::MachineMeta, view};

#[test]
fn session_rows_retain_the_launch_identity_and_older_rows_can_lack_it() {
    assert_eq!(
        view::row(&json!({"name": "codex/p/tag", "launch_id": "launch"}))
            .launch_id
            .as_deref(),
        Some("launch")
    );
    let alias = view::row(&json!({"name": "dsh/p/tag", "cmd": "dsh", "launch_id": "launch"}));
    assert_eq!(alias.agent, "deepseek");
    assert_eq!(
        alias.notify.cmd, "dsh",
        "binding compares the raw command, as Qt does"
    );
    for value in [
        json!({}),
        json!({"launch_id": null}),
        json!({"launch_id": 3}),
        json!({"launch_id": ""}),
    ] {
        assert_eq!(view::row(&value).launch_id, None);
    }
}

#[test]
fn an_empty_ended_launches_list_is_supported_and_absence_is_an_older_peer() {
    for value in [
        json!({}),
        json!({"ended_launches": null}),
        json!({"ended_launches": {}}),
    ] {
        assert_eq!(MachineMeta::project(&value).ended_launches, None);
    }
    assert_eq!(
        MachineMeta::project(&json!({"ended_launches": []})).ended_launches,
        Some(Vec::new())
    );
}

#[test]
fn ended_launches_keep_newest_first_nullable_exit_codes_and_a_bounded_list() {
    let rows: Vec<Value> = (0..33)
        .map(|n| {
            json!({"launch_id": format!("launch-{n}"), "name": "codex/p/tag",
        "code": if n == 0 { json!(null) } else { json!(n) }, "output": "retained output"})
        })
        .collect();
    let machine = MachineMeta::project(&json!({"ended_launches": rows}));
    let launches = machine.ended_launches.as_ref().unwrap();
    assert_eq!(launches.len(), 32);
    assert_eq!(launches[0].launch_id, "launch-0");
    assert_eq!(launches[0].code, None);
    assert_eq!(launches[31].code, Some(31));
    assert_eq!(launches[31].output, "retained output");
    let encoded = serde_json::to_value(&machine).unwrap();
    assert_eq!(encoded["ended_launches"][0]["code"], Value::Null);
    assert_eq!(
        serde_json::from_value::<MachineMeta>(encoded).unwrap(),
        machine
    );
}
