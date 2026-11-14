//! A marked DeepSeek claim is proven by the hosted caller proof and by
//! nothing else (zerus-63rru.28): its session's key when the host's per-call
//! token names the conversation, else a refusal in the proof's own words.
//! The ancestry proof is never read after it. Each case runs in an isolated
//! child with a stand-in host (`hosted_fixture`).
use super::*;
use crate::teams::caller::hosted_fixture::{isolated, Answer, Host, NAME, TOKEN};
use zerus_team_plan::model::SessionKey;

const LAUNCH: &str = "00000000-0000-4000-8000-0000000000e1";

#[test]
fn a_hosted_tool_call_is_its_session_and_never_the_ancestry_proof() {
    isolated("teams::caller::tests::hosted::proven_fixture");
}

#[test]
fn every_hosted_refusal_keeps_the_proofs_own_sentence() {
    isolated("teams::caller::tests::hosted::refusal_fixture");
}

/// The session key a proven claim must give, on this computer.
fn key(config: &Config, session_id: String) -> Caller {
    Caller::Session(SessionKey {
        machine: crate::state::session_graph::caller_machine(config).unwrap(),
        session_id,
    })
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn proven_fixture() {
    let config = Config::load().unwrap();
    let host = Host::start();
    let reads = crate::state::session_graph::caller_binding_reads();
    // A member Teams launched: its launch ID, by the session index and by
    // the label alike.
    let member = host.launched(NAME, Some(LAUNCH));
    host.call(&member);
    assert_eq!(caller(&config), key(&config, format!("launch_id:{LAUNCH}")));
    std::env::remove_var("HGS_SESSION_ID");
    assert_eq!(caller(&config), key(&config, format!("launch_id:{LAUNCH}")));
    // A session opened elsewhere has no launch ID: its run ID.
    let other = host.launched("dsh/example/opened", None);
    host.call(&other);
    let run = other["run_id"].as_str().unwrap();
    assert_eq!(caller(&config), key(&config, format!("run_id:{run}")));
    assert_eq!(
        crate::state::session_graph::caller_binding_reads(),
        reads,
        "the ancestry proof's binding read is never made"
    );
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn refusal_fixture() {
    let config = Config::load().unwrap();
    let host = Host::start();
    let mut member = host.launched(NAME, Some(LAUNCH));
    host.call(&member);
    let reads = crate::state::session_graph::caller_binding_reads();
    let refused = |expected: &str| match caller(&config) {
        Caller::Refused(problem) => {
            assert_eq!(problem.code, code::NOT_ALLOWED, "{expected}");
            assert_eq!(problem.message, expected);
        }
        other => panic!("{expected}: {other:?}"),
    };
    // The host must know this token for this conversation.
    host.answer(Answer::Older);
    refused("The running DeepSeek host predates per-call tokens; its tool calls cannot be proven.");
    host.answer(Answer::Unknown);
    refused("The DeepSeek host knows no live tool call with this token.");
    host.answer(Answer::Other);
    refused("The DeepSeek host holds this tool call's token for another conversation.");
    host.answer(Answer::Silent);
    refused("The DeepSeek host did not confirm this tool call in time.");
    host.answer(Answer::Tokens);
    assert!(matches!(caller(&config), Caller::Session(_)));
    // The call's own token and home.
    std::env::remove_var("HGS_DSH_CALL_TOKEN");
    refused("This command carries no DeepSeek per-call token; only a tool call of a host started by this hgs version can be proven.");
    std::env::set_var("HGS_DSH_CALL_TOKEN", TOKEN);
    std::env::set_var("DSH_HOME", format!("{}-other", host.home));
    refused("This command's DeepSeek home is not the session's agent home.");
    std::env::set_var("DSH_HOME", &host.home);
    // The binding the markers name.
    std::env::set_var("HGS_RUN_ID", "another-run");
    refused("Cannot verify the DeepSeek session binding the markers name.");
    host.call(&member);
    // The recorded host of this boot.
    let record = host.dir.join("host.identity.json");
    let saved = std::fs::read(&record).unwrap();
    std::fs::remove_file(&record).unwrap();
    refused("No live DeepSeek host is recorded for the session's agent home.");
    std::fs::write(&record, saved).unwrap();
    assert!(matches!(caller(&config), Caller::Session(_)));
    // An archived binding is established and not live.
    member["state"] = json!("archived");
    crate::state::dsh::save_binding_for_test(&mut member).unwrap();
    refused(&not_allowed().message);
    assert_eq!(
        crate::state::session_graph::caller_binding_reads(),
        reads,
        "no refusal falls back to the ancestry proof"
    );
}

/// A wrong session-ID hint beside an otherwise valid label, run and token:
/// the hint selects the binding, so another session's ID, or one that no
/// session has, is refused and never read past to the label.
#[test]
fn a_wrong_session_id_beside_a_valid_label_run_and_token_is_refused() {
    isolated("teams::caller::tests::hosted::wrong_session_id_fixture");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn wrong_session_id_fixture() {
    let config = Config::load().unwrap();
    let host = Host::start();
    let member = host.launched(NAME, Some(LAUNCH));
    let other = host.launched(
        "dsh/example/other",
        Some("00000000-0000-4000-8000-0000000000e2"),
    );
    host.call(&member);
    assert_eq!(caller(&config), key(&config, format!("launch_id:{LAUNCH}")));
    let others = other["session_id"].as_str().unwrap().to_owned();
    assert_ne!(Some(others.as_str()), member["session_id"].as_str());
    for wrong in [others.as_str(), "00000000-0000-4000-8000-0000000000ee"] {
        std::env::set_var("HGS_SESSION_ID", wrong);
        let answer = caller(&config);
        println!("WRONG_SESSION_ID {wrong} {answer:?}");
        assert_eq!(
            answer,
            Caller::Refused(Problem::new(
                code::NOT_ALLOWED,
                "Cannot verify the DeepSeek session binding the markers name."
            )),
            "{wrong}"
        );
    }
}

/// A host whose `ping` advertises call tokens under a protocol other than 1
/// does not advertise them: its tool calls cannot be proven.
#[test]
fn a_host_advertising_call_tokens_under_another_protocol_is_refused() {
    isolated("teams::caller::tests::hosted::other_protocol_fixture");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn other_protocol_fixture() {
    let config = Config::load().unwrap();
    let host = Host::start();
    let member = host.launched(NAME, Some(LAUNCH));
    host.call(&member);
    assert_eq!(caller(&config), key(&config, format!("launch_id:{LAUNCH}")));
    host.answer(Answer::OtherProtocol);
    let answer = caller(&config);
    println!("OTHER_PROTOCOL {answer:?}");
    assert_eq!(
        answer,
        Caller::Refused(Problem::new(
            code::NOT_ALLOWED,
            "The running DeepSeek host predates per-call tokens; its tool calls cannot be proven."
        ))
    );
}
