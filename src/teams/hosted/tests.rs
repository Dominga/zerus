//! The host state of an agent home, against a stand-in host this test process
//! serves (`hosted_fixture`), in an isolated child.
use super::*;
use crate::teams::caller::hosted_fixture::{isolated, Answer, Host};

#[test]
fn the_host_state_comes_from_the_recorded_live_hosts_own_ping() {
    isolated("teams::hosted::tests::state_fixture");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn state_fixture() {
    let home = std::env::var("DSH_HOME").unwrap();
    // No host is recorded for the home: a launch would start one.
    assert_eq!(deepseek_host(&home), Ok(HostState::NoHost));
    let host = Host::start();
    assert_eq!(deepseek_host(&home), Ok(HostState::Proves));
    host.answer(Answer::Older);
    assert_eq!(deepseek_host(&home), Ok(HostState::Older));
    host.answer(Answer::Silent);
    let started = std::time::Instant::now();
    let problem = deepseek_host(&home).unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert!(
        problem.message.contains("did not answer in time"),
        "{problem:?}"
    );
    host.answer(Answer::Tokens);
    // Another home's record names nothing for this one.
    assert_eq!(
        deepseek_host(&format!("{home}-other")),
        Ok(HostState::NoHost)
    );
    // A recorded process that has exited is no host.
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    crate::state::dsh::record_this_boot_for_test(&host.dir, other.id()).unwrap();
    // While it runs, the socket's peer is not that process: no answer counts.
    let problem = deepseek_host(&home).unwrap_err();
    assert!(
        problem.message.contains("not served by the recorded host"),
        "{problem:?}"
    );
    let _ = other.kill();
    let _ = other.wait();
    assert_eq!(deepseek_host(&home), Ok(HostState::NoHost));
    crate::state::dsh::record_this_boot_for_test(&host.dir, std::process::id()).unwrap();
    assert_eq!(deepseek_host(&home), Ok(HostState::Proves));
    // A record that cannot be read is no state.
    std::fs::write(host.dir.join("host.identity.json"), b"{").unwrap();
    assert!(deepseek_host(&home).is_err());
}

#[test]
fn an_older_host_is_refused_with_a_sentence_a_person_can_act_on() {
    let problem = older("/work/example/.dsh");
    assert_eq!(problem.code, code::INVALID_STATE);
    assert_eq!(
        problem.message,
        "The DeepSeek host that serves /work/example/.dsh lacks the caller, task-input or lifecycle capabilities required by Agent Teams. They become available after its next own start, which Zerus never forces."
    );
}

/// A host whose `ping` advertises call tokens under a protocol other than 1
/// does not advertise them: the member launch's check finds it older, as it
/// finds a host without them (the review's mutant W7, zerus-63rru.28).
#[test]
fn a_host_advertising_call_tokens_under_another_protocol_is_older() {
    isolated("teams::hosted::tests::other_protocol_fixture");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn other_protocol_fixture() {
    let home = std::env::var("DSH_HOME").unwrap();
    let host = Host::start();
    assert_eq!(deepseek_host(&home), Ok(HostState::Proves));
    host.answer(Answer::OtherProtocol);
    assert_eq!(deepseek_host(&home), Ok(HostState::Older));
}
