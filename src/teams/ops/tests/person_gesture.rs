use super::*;

#[test]
fn person_origin_alone_cannot_decide_an_owner_card() {
    for origin in [PersonOrigin::CliInterim, PersonOrigin::DesktopUi] {
        let mut h = Harness::new();
        // Both the team and the card are created by the product's writers.
        let card = h
            .run(
                &service(),
                Operation::OpenCard {
                    kind: "launch_refused".into(),
                    text: "No account available".into(),
                    node: None,
                    decisions: None,
                    parameters: None,
                },
            )
            .unwrap()
            .item
            .unwrap();
        let before = h.store.card(TEAM, card).unwrap();
        let revision = h.store.team(TEAM).unwrap().unwrap().revision;
        let request = request(
            revision,
            Operation::DecideCard {
                card,
                decision: "acknowledged".into(),
            },
        );
        let claimed_person = Actor::Person {
            person: "local".into(),
            origin,
        };
        refused(
            super::super::apply(&mut h.store, &claimed_person, &request, 1234),
            "person_gesture_required",
        );
        assert_eq!(h.store.card(TEAM, card).unwrap(), before);
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, revision);
        let saved: u64 = h
            .store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM requests WHERE id=?1",
                [&request.request_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(saved, 0, "a missing gesture never mints a receipt");
    }
}

#[test]
fn missing_gesture_does_not_create_a_team_store() {
    let mut h = Harness::empty();
    let request = request(
        0,
        Operation::DecideCard {
            card: 1,
            decision: "acknowledged".into(),
        },
    );
    refused(
        super::super::apply(&mut h.store, &person(), &request, 1234),
        "person_gesture_required",
    );
    assert!(!h.temp.path().join("teams/teams-v1.sqlite3").exists());
}
