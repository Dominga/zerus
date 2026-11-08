use super::*;
use crate::state::teams::model::{TeamPause, TeamStatus};

/// Task #246: the owner dismisses a Waiting row; it leaves the live list and
/// the count, stays in the dismissed section, and comes back when its state
/// changes or on Show dismissed > Restore. Only the owner may dismiss.
#[test]
fn the_owner_dismisses_a_waiting_row_and_its_state_returns_it() {
    let (mut team, _, _) = super::support::review_team();
    team.status = TeamStatus::Paused;
    team.paused = Some(TeamPause { at: 100.0, by: "You".into(), ..Default::default() });
    super::support::with_review_team(&team, || {
        let view = super::support::call("waiting", json!({})).unwrap();
        let item = view["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["kind"] == "team_paused")
            .expect("the paused team waits")
            .clone();
        assert_eq!(item["state"], "paused:100.0", "{item}");
        // Dismiss it.
        let dismissed = super::support::call(
            "waiting",
            json!({"action":"dismiss","id":item["id"],"team":item["team"],"state":item["state"]}),
        )
        .unwrap();
        assert_eq!(dismissed["state"], item["state"]);
        let view = super::support::call("waiting", json!({})).unwrap();
        assert!(!view["items"].as_array().unwrap().iter().any(|other| other["kind"] == "team_paused"), "{view}");
        assert!(
            view["dismissed"].as_array().unwrap().iter().any(|other| other["kind"] == "team_paused" && other["state"] == item["state"]),
            "the dismissal stays in its own section: {view}"
        );
        assert!(view["total"].as_u64().unwrap_or_default() < 1 || !view["items"].as_array().unwrap().iter().any(|other| other["id"] == item["id"]));
        // Show dismissed > Restore brings it back.
        super::support::call("waiting", json!({"action":"undismiss","id":item["id"]})).unwrap();
        let view = super::support::call("waiting", json!({})).unwrap();
        assert!(view["items"].as_array().unwrap().iter().any(|other| other["kind"] == "team_paused"), "{view}");
        // Dismissed again, a second pause is a new state: the row returns.
        super::support::call(
            "waiting",
            json!({"action":"dismiss","id":item["id"],"team":item["team"],"state":item["state"]}),
        )
        .unwrap();
        let mut store = Store::open(true).unwrap();
        store
            .mutate(None, "rv", None, |team, _events| {
                team.paused = Some(TeamPause { at: 200.0, ..Default::default() });
                Ok(Value::Null)
            })
            .unwrap();
        let view = super::support::call("waiting", json!({})).unwrap();
        assert!(
            view["items"].as_array().unwrap().iter().any(|other| other["kind"] == "team_paused" && other["state"] == "paused:200.0"),
            "{view}"
        );
        assert!(!view["dismissed"].as_array().unwrap().iter().any(|other| other["kind"] == "team_paused"), "{view}");
        // Only the owner dismisses.
        let error = super::support::call(
            "waiting",
            json!({"action":"dismiss","id":item["id"],"state":item["state"],"by":"lead"}),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("Only you dismiss a Waiting row"), "{error}");
    });
}

/// Task #246: a missing state is refused: the dismissal would hide the row
/// whatever happens next.
#[test]
fn a_dismissal_needs_the_state_it_saw() {
    let (team, _, _) = super::support::review_team();
    super::support::with_review_team(&team, || {
        let error = super::support::call("waiting", json!({"action":"dismiss","id":"rv/team_paused/100"})).unwrap_err().to_string();
        assert!(error.contains("needs its state"), "{error}");
    });
}
