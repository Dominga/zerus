//! A marked claim that is not a DeepSeek claim, made below a recorded
//! resident agent host, is refused with a sentence of its own
//! (zerus-63rru.28): there a command can only be a session that host serves,
//! and only a DeepSeek claim's per-call token proves one. The fact is the
//! process tree and the host records, never a marker the caller sets.
//!
//! Each case is a real tree. The test runner is the claimed agent, its
//! session saved by the state's own writer; an isolated child is the
//! stand-in host, recorded by the host's production-typed writer and serving
//! the host socket; and that child's own child makes the claim.
use super::*;
use crate::teams::caller::hosted_fixture::{below, isolated, parent_session, Host, NAME};
use std::path::PathBuf;

const LAUNCH: &str = "00000000-0000-4000-8000-0000000000e1";
const ANSWER: &str = "teams::caller::tests::under_host::answer";
const BELOW_HOST: &str = "This command runs inside an agent host's process tree, where a command can only be a session that host serves; session markers that name another session cannot be proven here.";
/// The Claude and the Codex claim: agent, name, run and launch ID.
const CLAIMS: [(&str, &str, &str, &str); 2] = [
    (
        "claude",
        "claude/example/member",
        "run-claude",
        "00000000-0000-4000-8000-0000000000c1",
    ),
    (
        "codex",
        "codex/example/member",
        "run-codex",
        "00000000-0000-4000-8000-0000000000c2",
    ),
];

/// What the command below answered, with the reads it made.
#[derive(Debug)]
struct Below {
    answer: String,
    host_reads: usize,
    binding_reads: usize,
    walks: usize,
}

/// The fields the command below printed. Its first line follows the test
/// harness's own "test ... " on the same line.
fn parse(text: &str) -> Below {
    let field = |key: &str| {
        text.lines()
            .find_map(|line| line.split_once(key).map(|(_, value)| value))
            .unwrap_or_else(|| panic!("{key}: {text}"))
            .trim()
            .to_owned()
    };
    Below {
        answer: field("ANSWER "),
        host_reads: field("HOST_READS ").parse().unwrap(),
        binding_reads: field("BINDING_READS ").parse().unwrap(),
        walks: field("WALKS ").parse().unwrap(),
    }
}

/// The claim of one session from a child of this process, carrying that
/// session's markers and, as a caller may, its agent's `HGS_AGENT`.
fn claim((agent, name, run, id): (&str, &str, &str, &str)) -> Below {
    let text = below(
        ANSWER,
        &[
            ("HGS_SESSION", name),
            ("HGS_RUN_ID", run),
            ("HGS_SESSION_ID", id),
            ("HGS_AGENT", agent),
        ],
    );
    let below = parse(&text);
    println!("CLAIM {agent} {below:?}");
    below
}

fn session(id: &str) -> String {
    format!("session:launch_id:{id}")
}

fn refused(problem: Problem) -> String {
    format!("refused:{}:{}", problem.code, problem.message)
}

fn below_host() -> String {
    refused(Problem::new(code::NOT_ALLOWED, BELOW_HOST))
}

/// The command below: what the caller proof decides, and its reads.
#[test]
#[ignore = "run below a stand-in host by its isolated fixture"]
fn answer() {
    let config = Config::load().unwrap();
    let answer = match caller(&config) {
        Caller::Person => "person".to_owned(),
        Caller::Session(key) => format!("session:{}", key.session_id),
        Caller::Refused(problem) => refused(problem),
    };
    println!("ANSWER {answer}");
    println!(
        "HOST_READS {}",
        crate::state::session_graph::caller_host_reads()
    );
    println!(
        "BINDING_READS {}",
        crate::state::session_graph::caller_binding_reads()
    );
    println!("WALKS {}", ancestry::walks());
}

/// A DeepSeek host recorded for another agent home: a sleeping child of this
/// process, so a sibling of the command, never its ancestor.
struct Elsewhere(std::process::Child);

impl Elsewhere {
    fn record() -> Self {
        let child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let dir = hosts().join(crate::state::dsh::host_key("/nonexistent/other-home"));
        crate::platform::private_dir(&dir).unwrap();
        crate::state::dsh::record_this_boot_for_test(&dir, child.id()).unwrap();
        Self(child)
    }
}

impl Drop for Elsewhere {
    fn drop(&mut self) {
        // Only this fixture's own child, by its handle.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn hosts() -> PathBuf {
    PathBuf::from(std::env::var("HGS_STATE_DIR").unwrap()).join("dsh/hosts")
}

/// The ancestry proof accepted these claims below a host; they are refused
/// now, whatever the claim's own markers say, `HGS_AGENT` among them. With
/// the host's record gone, the very same claims are proven.
#[test]
fn a_claude_or_codex_claim_below_a_recorded_host_is_refused() {
    isolated("teams::caller::tests::under_host::claims_below_the_host");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn claims_below_the_host() {
    let host = Host::start();
    let answers: Vec<_> = CLAIMS
        .into_iter()
        .map(|claimed| {
            parent_session(claimed.0, claimed.1, claimed.2, claimed.3);
            claim(claimed).answer
        })
        .collect();
    assert_eq!(answers, [below_host(), below_host()]);
    std::fs::remove_file(host.dir.join("host.identity.json")).unwrap();
    for claimed in CLAIMS {
        assert_eq!(claim(claimed).answer, session(claimed.3), "{}", claimed.0);
    }
}

/// The stand-in host: it waits for its launcher's go, then makes the call
/// as its own child, with the environment it inherited.
const WAIT_THEN_CALL: &str = "n=0; while [ ! -f \"$2\" ] && [ $n -lt 2000 ]; do sleep 0.01; n=$((n+1)); done; \"$0\" --exact \"$1\" --ignored --test-threads=1 --nocapture; exit $?";

/// The real case: `hgs` starts a DeepSeek host with `spawn_detached`, which
/// only calls `setsid`, so the host stays a child of the launching command
/// for as long as that command lives and inherits the launching agent's
/// markers. A tool call under it carries those markers unchanged, and the
/// ancestry proof took it for the launching session. Here the launching
/// command is this isolated child, below the launching session's agent (the
/// test runner): it starts the stand-in host with the production
/// `spawn_detached` and records it with the host's production-typed writer.
/// Without the record, the very same call is proven.
#[test]
fn a_call_under_a_host_its_launching_command_still_holds_is_refused() {
    isolated("teams::caller::tests::under_host::launching_command");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn launching_command() {
    let home = std::env::var("DSH_HOME").unwrap();
    let dir = hosts().join(crate::state::dsh::host_key(&home));
    for (recorded, (agent, name, run, id)) in
        [(true, CLAIMS[0]), (true, CLAIMS[1]), (false, CLAIMS[0])]
    {
        parent_session(agent, name, run, id);
        // The launching command runs in the agent's shell, with its markers.
        std::env::set_var("HGS_SESSION", name);
        std::env::set_var("HGS_RUN_ID", run);
        let _ = std::fs::remove_dir_all(&dir);
        crate::platform::private_dir(&dir).unwrap();
        let go = dir.join("go");
        let mut host = std::process::Command::new("/bin/sh");
        host.args(["-c", WAIT_THEN_CALL])
            .arg(std::env::current_exe().unwrap())
            .arg(ANSWER)
            .arg(&go)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let child = crate::platform::spawn_detached(&mut host).unwrap();
        if recorded {
            crate::state::dsh::record_this_boot_for_test(&dir, child.id()).unwrap();
        }
        std::fs::write(&go, b"").unwrap();
        let output = child.wait_with_output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{text}");
        let below = parse(&text);
        println!("CLAIM {agent} under the host its launching command holds, recorded={recorded} {below:?}");
        if recorded {
            assert_eq!(below.answer, below_host(), "{agent}");
        } else {
            assert_eq!(below.answer, session(id), "{agent}: no host recorded");
        }
    }
}

/// The host's own session: a DeepSeek claim with its per-call token is
/// proven by the hosted proof, below the same host.
#[test]
fn a_deepseek_claim_with_its_token_below_its_host_is_still_proven() {
    isolated("teams::caller::tests::under_host::deepseek_below_its_host");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn deepseek_below_its_host() {
    let host = Host::start();
    let member = host.launched(NAME, Some(LAUNCH));
    host.call(&member);
    let below = parse(&below(ANSWER, &[("HGS_AGENT", "dsh")]));
    println!("CLAIM dsh {below:?}");
    assert_eq!(below.answer, session(LAUNCH));
    assert_eq!(
        (below.walks, below.binding_reads),
        (0, 0),
        "the hosted proof alone"
    );
}

/// A Claude or Codex member's own command with no host above it is proven,
/// whether this computer records no host or one elsewhere.
#[test]
fn a_claude_or_codex_member_with_no_host_above_is_still_proven() {
    isolated("teams::caller::tests::under_host::no_host_above");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn no_host_above() {
    for claimed in CLAIMS {
        parent_session(claimed.0, claimed.1, claimed.2, claimed.3);
        assert_eq!(claim(claimed).answer, session(claimed.3), "{}", claimed.0);
    }
    let _elsewhere = Elsewhere::record();
    for claimed in CLAIMS {
        assert_eq!(claim(claimed).answer, session(claimed.3), "{}", claimed.0);
    }
}

/// The shared host check reads no Teams ancestor environment. A claim
/// refused below a host never enters the claimed binding proof.
#[test]
fn a_marked_claim_uses_the_shared_host_proof_without_reading_a_binding_under_it() {
    isolated("teams::caller::tests::under_host::reads_in_order");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn reads_in_order() {
    let claimed = CLAIMS[0];
    parent_session(claimed.0, claimed.1, claimed.2, claimed.3);
    let none = claim(claimed);
    assert_eq!(none.answer, session(claimed.3));
    assert_eq!((none.host_reads, none.walks), (0, 0), "no host recorded");
    let _elsewhere = Elsewhere::record();
    let other = claim(claimed);
    assert_eq!(other.answer, session(claimed.3));
    assert_eq!((other.host_reads, other.walks), (1, 0), "the shared walk, no Teams environment walk");
    let _host = Host::start();
    let under = claim(claimed);
    assert_eq!(under.answer, below_host());
    assert_eq!(
        (under.host_reads, under.walks, under.binding_reads),
        (2, 0, 0),
        "below a host"
    );
}

/// Host records that cannot be read, or more of them than the bound, refuse
/// a marked claim with the person path's own sentences: the check cannot
/// tell whether a host is above.
#[test]
fn unreadable_or_unbounded_host_records_refuse_a_marked_claim() {
    isolated("teams::caller::tests::under_host::fail_closed");
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn fail_closed() {
    let claimed = CLAIMS[0];
    parent_session(claimed.0, claimed.1, claimed.2, claimed.3);
    let hosts = hosts();
    // A corrupt record.
    let corrupt = hosts.join("corrupt");
    crate::platform::private_dir(&corrupt).unwrap();
    std::fs::write(corrupt.join("host.identity.json"), "{").unwrap();
    let answer = claim(claimed).answer;
    assert!(answer.starts_with("refused:invalid_state:"), "{answer}");
    assert!(answer.contains("DeepSeek agent host record"), "{answer}");
    std::fs::remove_dir_all(&corrupt).unwrap();
    // A host that names its process ID without a birth, as its own
    // `host.pid` does, and is above the command: no birth can tell it apart.
    let legacy = hosts.join("legacy");
    crate::platform::private_dir(&legacy).unwrap();
    std::fs::write(legacy.join("host.pid"), std::process::id().to_string()).unwrap();
    let answer = claim(claimed).answer;
    assert!(answer.starts_with("refused:invalid_state:"), "{answer}");
    assert!(answer.contains("DeepSeek agent host record"), "{answer}");
    std::fs::remove_dir_all(&legacy).unwrap();
    // One host folder more than the 64 the check reads.
    for n in 0..=64 {
        std::fs::create_dir(hosts.join(format!("folder-{n}"))).unwrap();
    }
    let answer = claim(claimed).answer;
    assert!(answer.contains("more DeepSeek agent host folders than the check reads"), "{answer}");
    for n in 0..=64 {
        std::fs::remove_dir(hosts.join(format!("folder-{n}"))).unwrap();
    }
    // One entry more than the 1024 the listing reads.
    for n in 0..=1024 {
        std::fs::write(hosts.join(format!("stray-{n}")), b"").unwrap();
    }
    let answer = claim(claimed).answer;
    assert!(answer.contains("more than 1024 entries"), "{answer}");
    for n in 0..=1024 {
        std::fs::remove_file(hosts.join(format!("stray-{n}"))).unwrap();
    }
    // Readable again and no host above: proven.
    assert_eq!(claim(claimed).answer, session(claimed.3));
}

/// The host check walks the process table alone: a shell between the host
/// and the command whose environment is over the 1 MiB the person path's
/// walk reads does not stop a marked claim. macOS cannot start a process with
/// such an environment, so this runs on Linux.
#[cfg(target_os = "linux")]
#[test]
fn the_host_check_reads_no_ancestors_environment() {
    isolated("teams::caller::tests::under_host::environment_unread");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "run by its isolated wrapper"]
fn environment_unread() {
    let (agent, name, run, id) = CLAIMS[0];
    parent_session(agent, name, run, id);
    let _elsewhere = Elsewhere::record();
    let mut shell = std::process::Command::new("/bin/sh");
    shell
        .args([
            "-c",
            "for n in $(seq 0 17); do unset PAD_$n; done; \"$0\" --exact \"$1\" --ignored --test-threads=1 --nocapture; exit $?",
        ])
        .arg(std::env::current_exe().unwrap())
        .arg(ANSWER)
        .envs([
            ("HGS_SESSION", name),
            ("HGS_RUN_ID", run),
            ("HGS_SESSION_ID", id),
        ]);
    for n in 0..18 {
        shell.env(format!("PAD_{n}"), "x".repeat(64_000));
    }
    let output = shell.output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    let below = parse(&text);
    println!("CLAIM {agent} below an oversized shell {below:?}");
    assert_eq!(below.answer, session(id));
    assert_eq!(below.walks, 0, "the shared process-only walk reads no Teams environment");
}
