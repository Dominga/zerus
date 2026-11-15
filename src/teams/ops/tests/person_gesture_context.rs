//! The provider-config merge must retain the closed Person writer admission.
use super::*;

#[test]
fn a_provider_context_cannot_authorize_a_person_owner_card_decision() {
    let mut h = Harness::new();
    let card = h
        .run(
            &service(),
            Operation::OpenCard {
                kind: "launch_refused".into(),
                text: "The account is unavailable.".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .unwrap()
        .item
        .unwrap();
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    let original = h.store.card(TEAM, card).unwrap();
    let request = request(
        revision,
        Operation::DecideCard {
            card,
            decision: "acknowledged".into(),
        },
    );
    let config = crate::config::Config {
        home: String::new(),
        dir: h.temp.path().join("config"),
        state: h.temp.path().to_owned(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    for context in [None, Some(&config)] {
        refused(
            super::super::apply_with_config(&mut h.store, &person(), &request, 1234, context),
            "person_gesture_required",
        );
    }
    assert_eq!(h.store.card(TEAM, card).unwrap(), original);
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, revision);
    let saved: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM requests WHERE id=?1",
            [request.request_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        saved, 0,
        "a Config never mints a person's gesture or a receipt"
    );
}
