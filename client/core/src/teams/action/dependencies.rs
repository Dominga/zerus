//! Wire coverage for the person's dependency-edit intent.
use super::TeamAction;
use serde_json::json;

#[test]
fn set_deps_names_the_cli_command_and_keeps_every_edge_kind() {
    let action: TeamAction = serde_json::from_value(json!({"kind":"set_deps", "node":3,
        "deps":[{"node":1,"kind":"soft"},{"node":2}]}))
    .unwrap();
    action.check().unwrap();
    assert_eq!(action.op(), "set-deps");
    assert_eq!(
        serde_json::to_value(action.input()).unwrap(),
        json!({"node":3,"deps":[{"node":1,"kind":"soft"},{"node":2}]})
    );
}

#[test]
fn set_deps_requires_an_explicit_list_and_valid_node_ids_and_bound() {
    assert!(serde_json::from_value::<TeamAction>(json!({"kind":"set_deps","node":1})).is_err());
    for value in [
        json!({"kind":"set_deps","node":0,"deps":[]}),
        json!({"kind":"set_deps","node":1,"deps":[{"node":0}]}),
        json!({"kind":"set_deps","node":1,"deps":vec![json!({"node":2});201]}),
    ] {
        assert!(serde_json::from_value::<TeamAction>(value)
            .unwrap()
            .check()
            .is_err());
    }
    serde_json::from_value::<TeamAction>(json!({"kind":"set_deps","node":1,"deps":[]}))
        .unwrap()
        .check()
        .unwrap();
}
