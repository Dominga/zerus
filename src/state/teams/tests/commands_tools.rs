//! Task #220: a missing-tool offer from a team's run goes to its lead first;
//! the lead denies it or asks the owner on a card.
use super::support::*;
use super::*;
use crate::state::teams::dispatcher::{fake_output, set_fake_hgs};
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

#[test]
fn an_attributed_offer_waits_for_its_lead_not_the_owner() {
    let state = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let id = stored_review_team();
        let project = tempfile::tempdir().unwrap();
        as_qa(|| tools::record(project.path(), &[(need("sccache"), false)])).unwrap();
        // The owner's Waiting for you does not carry a team's offer.
        assert!(tools::waiting_items().is_empty(), "{:?}", tools::waiting_items());
        // The lead's own inbox does, with the Deny and the Ask the owner commands.
        let inbox = run("inbox", &json!({"team":id,"by":"human"}), false).unwrap();
        let items = inbox["items"].as_array().unwrap();
        let item = items.iter().find(|item| item["kind"] == "tool_missing").expect("the lead's inbox names it");
        let command = item["command"].as_str().unwrap_or_default();
        assert!(command.contains("decide-tool") && command.contains("\"allow\":false"), "{item}");
        let detail = item["detail"].as_str().unwrap_or_default();
        assert!(detail.contains("install-tool") && detail.contains("hgs team propose"), "{item}");
        assert_eq!(item["task"], json!(2));
        // An offer no team raised keeps the owner's path.
        let alone = tempfile::tempdir().unwrap();
        tools::record(alone.path(), &[(need("ccache"), false)]).unwrap();
        let owner = tools::waiting_items();
        assert_eq!((owner.len(), owner[0]["tool"].as_str()), (1, Some("ccache")), "{owner:?}");
    });
}

#[test]
fn the_lead_denies_its_own_teams_offer_and_never_allows() {
    let state = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let id = stored_review_team();
        let project = tempfile::tempdir().unwrap();
        as_qa(|| tools::record(project.path(), &[(need("sccache"), false)])).unwrap();
        let offer = tools::offers_of(&id)[0].id.clone();
        // The member cannot decide it.
        let error = as_qa(|| call("decide-tool", json!({"offer":offer,"allow":false,"reason":"no"}))).unwrap_err().to_string();
        assert!(error.contains("Only the team's lead") || error.contains("agent session"), "{error}");
        // The lead cannot allow an install, and a Deny says why.
        let error = as_lead(|| call("decide-tool", json!({"offer":offer,"allow":true}))).unwrap_err().to_string();
        assert!(error.contains("never installs a tool"), "{error}");
        let error = as_lead(|| call("decide-tool", json!({"offer":offer,"allow":false}))).unwrap_err().to_string();
        assert!(error.contains("A Deny says why"), "{error}");
        let denied = as_lead(|| call("decide-tool", json!({"offer":offer,"allow":false,"reason":"we build without it"}))).unwrap();
        assert_eq!((denied["status"].as_str(), denied["tool"].as_str()), (Some("denied"), Some("sccache")), "{denied}");
        let stored = tools::offer(&offer).unwrap();
        assert_eq!((stored.state.as_str(), stored.reason.as_str()), ("denied", "we build without it"));
        // The lead's inbox no longer names it, and the owner's list never did.
        let inbox = run("inbox", &json!({"team":id,"by":"human"}), false).unwrap();
        assert!(inbox["items"].as_array().unwrap().iter().all(|item| item["kind"] != "tool_missing"), "{inbox}");
        assert!(tools::waiting_items().is_empty());
    });
}

#[test]
fn the_leads_card_installs_the_tool_or_keeps_the_owners_deny() {
    let state = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let id = stored_review_team();
        let project = tempfile::tempdir().unwrap();
        let (good, bad): (&[&str], &[&str]) = if cfg!(windows) {
            (&["cmd", "/C", "echo installed fine"], &["cmd", "/C", "echo no such package & exit /b 7"])
        } else {
            (&["sh", "-c", "echo installed fine"], &["sh", "-c", "echo no such package; exit 7"])
        };
        let sccache = Need { command: good.iter().map(|word| (*word).to_owned()).collect(), ..need("sccache") };
        let ccache = Need { command: bad.iter().map(|word| (*word).to_owned()).collect(), ..need("ccache") };
        as_qa(|| tools::record(project.path(), &[(sccache, false), (ccache, false)])).unwrap();
        let offers = tools::offers_of(&id);
        let of = |tool: &str| offers.iter().find(|offer| offer.tool == tool).unwrap().id.clone();
        let (sccache, ccache) = (of("sccache"), of("ccache"));
        // The lead asks the owner; the card names the tool, why and the command.
        let proposed = as_lead(|| call("propose", json!({"kind":"install-tool","offer":sccache,"reason":"the build cache needs it"}))).unwrap();
        let proposal = proposed["proposal"].as_str().unwrap().to_owned();
        let hash = proposed["hash"].as_str().unwrap().to_owned();
        let waiting = run("waiting", &json!({"by":"human"}), false).unwrap();
        let card = waiting["items"].as_array().unwrap().iter().find(|item| item["tool"] == "sccache").expect("the owner's card");
        assert_eq!((card["action"].as_str(), card["offer"].as_str()), (Some("decide_proposal"), Some(sccache.as_str())), "{card}");
        assert_eq!(card["install_command"], json!(good.join(" ")), "{card}");
        assert!(card["title"].as_str().unwrap().contains("install sccache"), "{card}");
        // The card answers the inbox item: sccache is not the lead's again
        // while the card is open; the other tool still waits for it.
        let inbox = run("inbox", &json!({"team":id,"by":"human"}), false).unwrap();
        let items = inbox["items"].as_array().unwrap();
        assert!(items.iter().all(|item| !(item["kind"] == "tool_missing" && item["summary"].as_str().unwrap_or_default().contains("sccache"))), "{inbox}");
        assert!(items.iter().any(|item| item["kind"] == "tool_missing" && item["summary"].as_str().unwrap_or_default().contains("ccache")), "{inbox}");
        // The lead cannot decide its own card, and an agent never installs.
        let error = as_lead(|| call("decide-proposal", json!({"proposal":proposal,"accept":true,"expected_hash":hash,"by":"human"})))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Only you"), "{error}");
        // The owner allows: the offer installs, and both sides read it.
        tools::inline_install(true);
        let allowed = call("decide-proposal", json!({"proposal":proposal,"accept":true,"expected_hash":hash,"by":"human"})).unwrap();
        tools::inline_install(false);
        assert_eq!(allowed["status"], "accepted", "{allowed}");
        let installed = tools::offer(&sccache).unwrap();
        assert_eq!((installed.state.as_str(), installed.exit), ("installed", Some(0)), "{installed:?}");
        let inbox = run("inbox", &json!({"team":id,"by":"human"}), false).unwrap();
        assert!(inbox["items"].as_array().unwrap().iter().any(|item| item["kind"] == "proposal_decided"), "{inbox}");
        // The owner denies the other card: the offer keeps the reason and stays out.
        let proposed = as_lead(|| call("propose", json!({"kind":"install-tool","offer":ccache,"reason":"the C++ cache needs it"}))).unwrap();
        let proposal = proposed["proposal"].as_str().unwrap().to_owned();
        let hash = proposed["hash"].as_str().unwrap().to_owned();
        call("decide-proposal", json!({"proposal":proposal,"accept":false,"expected_hash":hash,"reason":"we can build without it","by":"human"})).unwrap();
        let denied = tools::offer(&ccache).unwrap();
        assert_eq!((denied.state.as_str(), denied.reason.as_str()), ("denied", "we can build without it"));
        assert!(tools::offers_of(&id).is_empty());
    });
}

#[test]
fn the_member_whose_run_asked_hears_the_outcome() {
    let state = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(state.path(), || {
        let id = stored_review_team();
        let project = tempfile::tempdir().unwrap();
        let (good, bad): (&[&str], &[&str]) = if cfg!(windows) {
            (&["cmd", "/C", "echo installed fine"], &["cmd", "/C", "echo no such package & exit /b 7"])
        } else {
            (&["sh", "-c", "echo installed fine"], &["sh", "-c", "echo no such package; exit 7"])
        };
        let sccache = Need { command: good.iter().map(|word| (*word).to_owned()).collect(), ..need("sccache") };
        let ccache = Need { command: bad.iter().map(|word| (*word).to_owned()).collect(), ..need("ccache") };
        as_qa(|| tools::record(project.path(), &[(sccache, false), (ccache, false), (need("gcovr"), false)])).unwrap();
        let offers = tools::offers_of(&id);
        let of = |tool: &str| offers.iter().find(|offer| offer.tool == tool).unwrap().id.clone();
        let (sccache, ccache, gcovr) = (of("sccache"), of("ccache"), of("gcovr"));
        // The member's session takes the messages; each one is kept here.
        let told = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let heard = told.clone();
        set_fake_hgs(Some(Box::new(move |_, args, input| {
            if args == ["--help"] {
                return Ok(fake_output(0, "", ""));
            }
            if let Some(input) = input {
                heard.borrow_mut().push(crate::state::teams::doorbell::posted_text(input));
            }
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        let told_any = |text: &str| told.borrow().iter().any(|sent| sent.contains(text));
        // The lead's Deny reaches the member.
        as_lead(|| call("decide-tool", json!({"offer":gcovr,"allow":false,"reason":"not needed here"}))).unwrap();
        assert!(told_any("denied: not needed here") && told_any("gcovr"), "{:?}", told.borrow());
        // The owner's Allow reaches it too.
        let proposed = as_lead(|| call("propose", json!({"kind":"install-tool","offer":sccache,"reason":"the build cache needs it"}))).unwrap();
        tools::inline_install(true);
        call("decide-proposal", json!({"proposal":proposed["proposal"],"accept":true,"expected_hash":proposed["hash"],"by":"human"})).unwrap();
        tools::inline_install(false);
        assert!(told_any("The owner allowed the install") && told_any("sccache"), "{:?}", told.borrow());
        // And the owner's Deny, with the reason.
        let proposed = as_lead(|| call("propose", json!({"kind":"install-tool","offer":ccache,"reason":"the C++ cache needs it"}))).unwrap();
        call("decide-proposal", json!({"proposal":proposed["proposal"],"accept":false,"expected_hash":proposed["hash"],"reason":"we build without it","by":"human"})).unwrap();
        assert!(told_any("denied: we build without it") && told_any("ccache"), "{:?}", told.borrow());
        set_fake_hgs(None);
    });
}
