

/// U62 (#715): only the owner grants a full test run, one release at a time.
#[test]
fn only_the_owner_grants_a_full_test_run() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let error = apply(&mut team, "full-tests", &json!({"release":"v1.57.0"}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Only you"), "{error}");
    let receipt = apply(&mut team, "full-tests", &json!({"release":"v1.57.0"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["release"].as_str()), (Some("granted"), Some("v1.57.0")));
    assert_eq!(team.full_tests_grant.as_ref().unwrap().release, "v1.57.0");
    assert!(events.iter().any(|event| event.kind == "full_tests_granted"));
    let revoked = apply(&mut team, "full-tests", &json!({"revoke":true}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(revoked["status"], "revoked");
    assert!(team.full_tests_grant.is_none());
}
