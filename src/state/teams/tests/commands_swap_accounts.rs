//! Owner, 2026-11-11 (team 86821caa): swapping three members to another
//! account left the team with nobody working.
//!
//! - The card named the account by its label ("additional"); it was accepted,
//!   allowed, and failed at the launch as "Unknown account profile", after the
//!   old sessions had been stopped. An account is named by id or label and
//!   checked at the proposal and before a card touches its first member.
//! - "Default" members already ran on the account the card named: a swap that
//!   changes nothing is refused instead of stopping a working session.
//! - Claude's usage-limit menu reached the lead with "Switch to usage
//!   credits" as an ordinary option; an option that pays is the owner's.

use super::support::*;
use super::*;

/// This computer's accounts, as on the owner's: the default for Claude is the
/// profile labelled "Additional"; the profile labelled "Default" is another.
fn resolve(provider: &str, name: &str) -> std::result::Result<String, String> {
    let accounts: &[(&str, &str, &str)] = &[
        ("claude", "native-claude", "Default account"),
        ("claude", "claude-58bd", "Additional"),
        ("claude", "claude-5758", "Default"),
        ("codex", "native-codex", "Default account"),
    ];
    accounts
        .iter()
        .find(|(agent, id, label)| *agent == provider && (*id == name || label.eq_ignore_ascii_case(name)))
        .map(|(_, id, _)| (*id).to_owned())
        .ok_or_else(|| format!("Unknown account \"{name}\" for {provider}: name an account's id or its label"))
}

fn effective(provider: &str, account: &str) -> String {
    match (provider, account) {
        ("claude", "") => "claude-58bd".into(),
        (_, "") => format!("native-{provider}"),
        (_, named) => named.to_owned(),
    }
}

fn book<'a>() -> AccountBook<'a> {
    AccountBook { local: &|computer| computer.is_empty(), resolve: &resolve, effective: &effective }
}

/// The running team with two Claude members beside dev: core on the default
/// account and world on "Default".
fn claude_team() -> Team {
    let mut team = running_team();
    for (id, account) in [("core", ""), ("world", "claude-5758")] {
        team.members.push(Member {
            id: id.into(),
            role: "Engineer".into(),
            agent: "claude".into(),
            account: account.into(),
            state: MemberState::Active,
            session: Some(SessionRef { name: format!("claude/x/{id}"), run_id: format!("r-{id}"), ..Default::default() }),
            ..Default::default()
        });
    }
    team
}

#[test]
fn a_swap_names_its_account_by_id_or_label() {
    let team = claude_team();
    // The label the owner sees, in any case, becomes the id the launch takes.
    for (given, id) in [("additional", "claude-58bd"), ("Additional", "claude-58bd"), ("claude-5758", "claude-5758"), ("Default", "claude-5758")] {
        let mut request = json!({"team":"t","member":"world","account":given,"model":"opus"});
        member_accounts(&team, "swap-member", &mut request, &book()).unwrap();
        assert_eq!(request["account"], id, "{given}");
    }
    // An unknown name is refused with the member and the field, before the
    // swap is applied; nothing about the request changes.
    let mut request = json!({"team":"t","member":"world","account":"spare"});
    let error = member_accounts(&team, "swap-member", &mut request, &book()).unwrap_err().to_string();
    assert!(error.contains("Member \"world\": field \"account\": Unknown account \"spare\" for claude"), "{error}");
    assert_eq!(request["account"], "spare");
    // The account is the new agent's when the swap changes the agent too.
    let mut request = json!({"team":"t","member":"world","agent":"codex","account":"Default account"});
    member_accounts(&team, "swap-member", &mut request, &book()).unwrap();
    assert_eq!(request["account"], "native-codex");
    // A request with no account, and another computer's member, are left.
    let mut request = json!({"team":"t","member":"world","model":"opus"});
    member_accounts(&team, "swap-member", &mut request, &book()).unwrap();
    assert!(request.get("account").is_none(), "{request}");
    let mut remote = claude_team();
    remote.members.iter_mut().find(|member| member.id == "world").unwrap().computer = "studio".into();
    let mut request = json!({"team":"t","member":"world","account":"spare"});
    member_accounts(&remote, "swap-member", &mut request, &book()).unwrap();
    assert_eq!(request["account"], "spare");
}

#[test]
fn a_card_is_checked_when_it_is_proposed() {
    let team = claude_team();
    // The three shapes a card takes: a list, one member with its fields beside
    // it, and one member object.
    let mut card = json!({"team":"t","kind":"swap-member","reason":"limit","members":[
        {"id":"world","account":"additional"},{"id":"dev","model":"gpt"}]});
    member_accounts(&team, "propose", &mut card, &book()).unwrap();
    assert_eq!(card["members"], json!([{"id":"world","account":"claude-58bd"},{"id":"dev","model":"gpt"}]));
    let mut card = json!({"team":"t","kind":"swap-member","reason":"limit","member":"world","account":"additional"});
    member_accounts(&team, "propose", &mut card, &book()).unwrap();
    assert_eq!(card["account"], "claude-58bd");
    let mut card = json!({"team":"t","kind":"swap-member","reason":"limit","member":{"id":"world","account":"additional"}});
    member_accounts(&team, "propose", &mut card, &book()).unwrap();
    assert_eq!(card["member"]["account"], "claude-58bd");
    // One unknown account refuses the whole card: the owner never sees it.
    let mut card = json!({"team":"t","kind":"swap-member","reason":"limit","members":[
        {"id":"world","account":"additional"},{"id":"core","account":"spare"}]});
    let error = member_accounts(&team, "propose", &mut card, &book()).unwrap_err().to_string();
    assert!(error.contains("Member \"core\"") && error.contains("spare"), "{error}");
    // A new member's account is named the same way.
    let mut card = json!({"team":"t","kind":"add-member","reason":"more hands","member":{"id":"ui","role":"UI","agent":"claude","account":"additional"}});
    member_accounts(&team, "propose", &mut card, &book()).unwrap();
    assert_eq!(card["member"]["account"], "claude-58bd");
    let mut request = json!({"team":"t","member":{"id":"ui","role":"UI","agent":"claude","account":"nope"}});
    let error = member_accounts(&team, "add-member", &mut request, &book()).unwrap_err().to_string();
    assert!(error.contains("Member \"ui\": field \"account\""), "{error}");
}

#[test]
fn a_swap_to_the_account_a_member_already_runs_on_is_refused() {
    let team = claude_team();
    // core runs on this computer's default for Claude, which is "Additional":
    // the swap would stop its session and keep the same credentials and limit.
    let mut card = json!({"team":"t","kind":"swap-member","reason":"limit","member":"core","account":"additional"});
    let error = member_accounts(&team, "propose", &mut card, &book()).unwrap_err().to_string();
    assert!(error.contains("Member \"core\" already runs on account claude-58bd (\"additional\"), this computer's default for claude"), "{error}");
    assert!(error.contains("change neither its credentials nor its usage limit"), "{error}");
    // A member that names the account says so without the default.
    let mut card = json!({"team":"t","kind":"swap-member","reason":"limit","member":"world","account":"claude-5758"});
    let error = member_accounts(&team, "propose", &mut card, &book()).unwrap_err().to_string();
    assert!(error.contains("already runs on account claude-5758:"), "{error}");
    // A new session on the same account is asked for in words; a swap that
    // changes something else as well is a real change.
    for extra in [json!({"fresh":true}), json!({"model":"opus"}), json!({"prompt":"Review only"})] {
        let mut card = json!({"team":"t","kind":"swap-member","reason":"limit","member":"core","account":"additional"});
        for (key, value) in extra.as_object().unwrap() {
            card[key] = value.clone();
        }
        member_accounts(&team, "propose", &mut card, &book()).unwrap();
        assert_eq!(card["account"], "claude-58bd");
    }
    // The owner's own swap is not a card: it applies as asked.
    let mut request = json!({"team":"t","member":"core","account":"additional"});
    member_accounts(&team, "swap-member", &mut request, &book()).unwrap();
    assert_eq!(request["account"], "claude-58bd");
}

#[test]
fn an_allowed_card_is_checked_whole_before_any_member_is_touched() {
    // A card stored before the check (or whose account has gone since) names
    // an account this computer does not have for one of its three members.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = claude_team();
    let proposal = json!({"kind":"swap-member","reason":"limit","members":[
        {"id":"world","account":"additional"},{"id":"core","account":"spare"},{"id":"dev","model":"gpt"}]});
    ops::apply(&mut team, "propose", &proposal, &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    set_test_accounts(Some((resolve, effective)));
    let decide = json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash});
    let error = decide_proposal_in(&mut store, &decide, &Actor::Human);
    set_test_accounts(None);
    let error = error.unwrap_err().to_string();
    assert!(error.contains("Member \"core\"") && error.contains("spare"), "{error}");
    // Every member keeps its session: world, named first, was not swapped.
    let team = store.load("t").unwrap().unwrap();
    for id in ["world", "core", "dev"] {
        let member = team.member(id).unwrap();
        assert!(member.session.is_some() && member.replaced.is_none(), "{id}: {:?}", member.session);
    }
    assert_eq!(team.member("world").unwrap().account, "claude-5758");
}

#[test]
fn an_option_that_pays_is_the_owners() {
    let card = json!({"source":"claude_usage_limit","questions":[{"id":"usage_limit","options":[
        {"id":"limit_0","label":"Stop and wait for limit to reset"},
        {"id":"limit_1","label":"Wait here, then continue automatically when the limit resets"},
        {"id":"limit_2","label":"Switch to usage credits"}]}]});
    let pick = |id: &str| json!([{"question_id":"usage_limit","selected_option_ids":[id]}]);
    assert_eq!(paid_limit_choice(&card, &pick("limit_2")).as_deref(), Some("Switch to usage credits"));
    assert_eq!(paid_limit_choice(&card, &pick("limit_0")), None);
    assert_eq!(paid_limit_choice(&card, &pick("limit_1")), None);
    // Another wording of the paid option stays the owner's.
    let mut extra = card.clone();
    extra["questions"][0]["options"][2]["label"] = json!("Switch to extra usage");
    assert!(paid_limit_choice(&extra, &pick("limit_2")).is_some());
    // An ordinary permission request is not a limit menu.
    let mut ordinary = card.clone();
    ordinary["source"] = json!("claude_tool_approval");
    assert_eq!(paid_limit_choice(&ordinary, &pick("limit_2")), None);
}
