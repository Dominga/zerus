use super::*;
use super::support::*;

/// U38: `profile` ranks a scope's losses read only; status carries it only
/// when asked; two phases compare side by side.
#[test]
fn profile_ranks_a_scope_and_status_carries_it_on_request() {
    let (team, _, _) = review_team();
    assert!(USAGE.contains("|profile|") && !MUTATIONS.contains(&"profile"));
    with_review_team(&team, || {
        let plan = run("profile", &json!({"team":"rv","tokens":false}), false).unwrap();
        assert_eq!(plan["scope"], json!({"plan":true}));
        assert!(plan["losses"]["by_time"]["items"].is_array() && plan["critical_path"]["planned"]["tasks"].is_array(), "{plan}");
        assert_eq!(plan["token_reads"]["read"], 0);
        let one = run("profile", &json!({"team":"rv","task":1,"tokens":false}), false).unwrap();
        assert_eq!(one["summary"]["tasks"], 1);
        let compared = run("profile", &json!({"team":"rv","compare":[1,2],"tokens":false}), false).unwrap();
        assert_eq!((compared["compare"]["a"]["phase"].clone(), compared["compare"]["b"]["phase"].clone()), (json!(1), json!(2)));
        assert!(run("profile", &json!({"team":"rv","compare":[1]}), false).is_err());
        let status = run("status", &json!({"team":"rv"}), false).unwrap();
        assert!(status.get("profile").is_none());
        let status = run("status", &json!({"team":"rv","profile":{"phase":2,"tokens":false}}), false).unwrap();
        assert_eq!(status["profile"]["scope"], json!({"phase":2}));
    });
}
