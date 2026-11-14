//! Product-written paging controls. No rows are inserted by hand.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::Operation;

#[test]
fn drawer_cursors_exclude_later_product_writes_and_other_lists() {
    let h = seeded();
    for kind in ["journal", "inbox", "cards"] {
        let (first, exit) = h.cli(
            &Caller::Person,
            &format!("{kind} --team {TEAM} --limit 1"),
            "",
        );
        assert_eq!(exit, 0, "{first}");
        // One seed card needs a second one to make a continuation cursor.
        if kind == "cards" {
            h.apply(
                &person(),
                Operation::OpenCard {
                    kind: "note".into(),
                    text: "Second reviewed note".into(),
                    node: None,
                    decisions: None,
                    parameters: None,
                },
            );
        }
        let (first, exit) = h.cli(
            &Caller::Person,
            &format!("{kind} --team {TEAM} --limit 1"),
            "",
        );
        assert_eq!(exit, 0, "{first}");
        let token = first["next"].as_str().unwrap().to_owned();
        let ceiling = crate::teams::cli::input::parse_cursor(&token)
            .unwrap()
            .ceiling;
        h.apply(
            &person(),
            Operation::OpenCard {
                kind: "note".into(),
                text: "Added after the page opened".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        );
        h.apply(
            &member("m1", M1_SESSION),
            Operation::Ask {
                node: 2,
                generation: 1,
                text: "Later question".into(),
            },
        );
        let (later, exit) = h.cli(
            &Caller::Person,
            &format!("{kind} --team {TEAM} --limit 200 --after {token}"),
            "",
        );
        assert_eq!(exit, 0, "{later}");
        assert!(!later["rows"].as_array().unwrap().is_empty());
        assert!(later["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["seq"].as_u64().unwrap() <= ceiling));
        let other = if kind == "inbox" { "cards" } else { "inbox" };
        let (foreign, exit) = h.cli(
            &Caller::Person,
            &format!("{other} --team {TEAM} --after {token}"),
            "",
        );
        assert_eq!((code(&foreign), exit), ("invalid_state", 1));
    }
}
