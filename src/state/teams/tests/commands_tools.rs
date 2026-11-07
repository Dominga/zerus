//! Task #220: a missing-tool offer from a team's run goes to its lead first;
//! the lead denies it or asks the owner on a card.
use super::support::*;
use super::*;
use crate::state::teams::model::SessionRef;
use crate::test_discipline::tools::{self, Need};

fn need(tool: &str) -> Need {
    Need {
        tool: tool.into(),
        why: format!("why {tool}"),
        saves: format!("saves {tool}"),
        source: "tool".into(),
        programs: vec![tool.into()],
        command: vec!["winget".into(), "install".into()],
    }
}

/// The review team stored with qa's session bound: qa runs task 2 in
/// `kimi/x/qa` (run r9), the lead in its dsh conversation.
fn stored_review_team() -> String {
    let (mut team, _, _) = review_team();
    let qa = team.members.iter_mut().find(|member| member.id == "qa").unwrap();
    qa.session = Some(SessionRef {
        host: String::new(),
        name: "kimi/x/qa".into(),
        run_id: "r9".into(),
        conversation_id: "qa-conversation".into(),
    });
    let id = team.id.clone();
    Store::open(true).unwrap().create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
    id
}

fn as_qa<T>(body: impl FnOnce() -> T) -> T {
    set_test_caller_identities(Some((None, Some("kimi/x/qa".into()), Some("r9".into()))));
    let result = body();
    set_test_caller_identities(Some((None, None, None)));
    result
}

fn as_lead<T>(body: impl FnOnce() -> T) -> T {
    set_test_caller_identities(Some((Some("lead-conversation".into()), None, None)));
    let result = body();
    set_test_caller_identities(Some((None, None, None)));
    result
}

#[test]
fn an_offer_a_teams_run_raises_names_its_team_member_and_task() {
    let state = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let id = stored_review_team();
        let project = tempfile::tempdir().unwrap();
        let offers = as_qa(|| tools::record(project.path(), &[(need("sccache"), false)])).unwrap();
        assert_eq!((offers[0].team.as_str(), offers[0].member.as_str(), offers[0].task), (id.as_str(), "qa", Some(2)), "{offers:?}");
        // The lead's own run names the team and the lead; the lead holds no task.
        let lead = as_lead(|| tools::record(project.path(), &[(need("ccache"), false)])).unwrap();
        let ccache = lead.iter().find(|offer| offer.tool == "ccache").unwrap();
        assert_eq!((ccache.team.as_str(), ccache.member.as_str(), ccache.task), (id.as_str(), "lead", None), "{lead:?}");
        // A run outside every team's session (the owner's shell, a worker's
        // run) keeps the owner's path: no team, no member, no task.
        let owner = tools::record(project.path(), &[(need("gcovr"), false)]).unwrap();
        let gcovr = owner.iter().find(|offer| offer.tool == "gcovr").unwrap();
        assert_eq!((gcovr.team.as_str(), gcovr.member.as_str(), gcovr.task), ("", "", None), "{owner:?}");
    });
}
