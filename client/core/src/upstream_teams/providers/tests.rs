use super::*;
fn ask() -> ConfigRead {
    ConfigRead {
        team: "00000000-0000-4000-8000-00000000beef".into(),
        candidate: None,
    }
}
fn exit(value: Value) -> Exit {
    Exit::Code {
        code: 0,
        stdout: serde_json::to_vec(&value).unwrap(),
        stderr: String::new(),
    }
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/providers.json")).unwrap()
}
#[test]
fn product_inspection_has_an_exact_scope_and_no_write_identity() {
    let ask = ask();
    let call = invocation(&ask).unwrap();
    assert_eq!(call.args, ["team", "validation-config", "--json"]);
    assert!(call.request_id.is_none());
    assert_eq!(
        serde_json::from_slice::<Value>(call.stdin.as_ref().unwrap()).unwrap(),
        json!({"team":ask.team})
    );
    let view = decode(&ask, exit(fixture())).unwrap();
    assert!(view.valid);
    assert_eq!(view.providers.len(), 2);
    assert_eq!(view.source, "detected");
}
#[test]
fn old_services_and_malformed_or_foreign_replies_stay_unavailable() {
    let ask = ask();
    let old = Exit::Code {
        code: 1,
        stdout: serde_json::to_vec(
            &json!({"ok":false,"error":{"code":"not_found","message":"Unknown command"}}),
        )
        .unwrap(),
        stderr: String::new(),
    };
    assert_eq!(decode(&ask, old).unwrap_err().code(), "not_found");
    for (field, value) in [
        ("schema", json!(2)),
        ("team", json!("00000000-0000-4000-8000-00000000aaaa")),
        ("digest", json!("invalid")),
        (
            "providers",
            json!(vec![fixture()["inspection"]["providers"][0].clone(); 17]),
        ),
    ] {
        let mut f = fixture();
        f["inspection"][field] = value;
        assert!(decode(&ask, exit(f)).is_err(), "{field}");
    }
}
#[test]
fn checked_override_cannot_be_replaced_by_a_different_answer() {
    let mut ask = ask();
    ask.candidate = Some(Candidate {
        override_config: Some(Config {
            schema: 1,
            provider: vec![],
        }),
    });
    assert!(decode(&ask, exit(fixture())).is_err());
}
#[test]
fn selection_action_keeps_original_digests_and_id() {
    use crate::upstream_teams::action::{prepare, TeamCall};
    let mut call = json!({"team":ask().team,"team_name":"Example","request_id":"00000000-0000-4000-8000-000000000001","expected_revision":1,"action":{"kind":"validation_config","selection":{"source_digest":"a".repeat(64),"digest":"b".repeat(64),"override_config":null}}});
    let native: TeamCall = serde_json::from_value(call.clone()).unwrap();
    let (inv, id) = prepare(&native).unwrap();
    assert_eq!(inv.args, ["team", "validation-settings", "--json"]);
    assert_eq!(id, call["request_id"]);
    let input: Value = serde_json::from_slice(&inv.stdin.unwrap()).unwrap();
    assert_eq!(input["selection"], call["action"]["selection"]);
    call["action"]["selection"]["digest"] = json!("bad");
    assert!(prepare(&serde_json::from_value(call).unwrap()).is_err());
}

#[test]
fn invalid_inspections_still_have_bounded_diagnostics_and_metadata() {
    for field in ["providers", "diagnostics", "unsupported"] {
        let mut value = fixture();
        value["inspection"]["valid"] = json!(false);
        let item = match field {
            "providers" => value["inspection"]["providers"][0].clone(),
            "diagnostics" => json!({"code":"invalid_config","message":"Malformed input"}),
            _ => json!("direct_runner"),
        };
        value["inspection"][field] = json!(vec![item; 17]);
        assert!(decode(&ask(), exit(value)).is_err(), "{field}");
    }
}
