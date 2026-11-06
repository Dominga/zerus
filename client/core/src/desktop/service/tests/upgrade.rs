use super::*;

fn upgrade(state: &str) -> Value {
    json!({"code":"hgs_replaced","state":state,"new_admission":false})
}

#[test]
fn upgrade_is_a_closed_bounded_projection_of_checked_service_status() {
    let mut value = status();
    value["executable_upgrade"] = upgrade("manual_refresh_required");
    value["executable_upgrade"]["untrusted_detail"] = json!("x".repeat(1024 * 1024));
    let mut service = state();
    service.frame(&frame(0, "hello", value.clone())).unwrap();
    assert_eq!(service.snapshot().epoch.as_deref(), Some(EPOCH));
    assert_eq!(
        service.snapshot().executable_upgrade,
        Some(ExecutableUpgrade::ManualRefreshRequired)
    );
    assert!(
        !service.snapshot().background_ready(),
        "display state grants no readiness"
    );
    assert_eq!(
        serde_json::to_value(service.snapshot()).unwrap()["executable_upgrade"],
        json!({"state":"manual_refresh_required"})
    );
    value["executable_upgrade"] = upgrade("draining");
    service.frame(&frame(1, "status", value)).unwrap();
    assert_eq!(
        service.snapshot().executable_upgrade,
        Some(ExecutableUpgrade::Draining)
    );
    service.frame(&frame(2, "heartbeat", status())).unwrap();
    assert!(service.snapshot().executable_upgrade.is_none());
}

#[test]
fn unknown_or_invalid_upgrade_fields_never_enter_the_projection() {
    for invalid in [
        Value::Null,
        json!([]),
        upgrade("unknown"),
        upgrade(&"x".repeat(4096)),
        json!({"code":"other","state":"manual_refresh_required","new_admission":false}),
        json!({"code":"hgs_replaced","state":"manual_refresh_required","new_admission":true}),
    ] {
        let mut value = status();
        value["executable_upgrade"] = invalid;
        let mut service = state();
        service.status(&value).unwrap();
        assert!(service.snapshot().connected);
        assert!(service.snapshot().executable_upgrade.is_none());
    }
}

#[test]
fn a_different_scope_cannot_inject_an_upgrade_notice() {
    let mut service = state();
    let mut value = status();
    value["executable_upgrade"] = upgrade("manual_refresh_required");
    service.status(&value).unwrap();
    value["configuration"] = json!("b".repeat(64));
    assert_eq!(service.status(&value), Err("incompatible_service"));
    assert!(service.snapshot().executable_upgrade.is_none());
    assert!(!service.snapshot().connected);
}
