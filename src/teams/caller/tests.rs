//! The caller proof: its parsing, its bound, and a real process tree. The
//! process cases start a command below a detached process that carries session
//! markers, with or without the live record that proves it.
use super::*;
use serde_json::json;

#[test]
fn the_immutable_identity_prefers_a_launch_id() {
    assert_eq!(
        identity(&json!({"launch_id": "l", "run_id": "r"})),
        "launch_id:l"
    );
    assert_eq!(identity(&json!({"run_id": "r"})), "run_id:r");
    assert_eq!(
        identity(&json!({"launch_id": "", "run_id": "r"})),
        "run_id:r"
    );
    assert_eq!(identity(&json!({})), "");
}

#[test]
fn markers_are_read_from_nul_separated_entries_and_need_both() {
    let both = b"HGS_SESSION=claude/example/one\0HGS_RUN_ID=run-1\0PATH=/bin";
    assert_eq!(
        markers(both),
        Some(("claude/example/one".into(), "run-1".into()))
    );
    assert_eq!(markers(b"HGS_SESSION=claude/example/one\0"), None);
    assert_eq!(markers(b"HGS_RUN_ID=run-1\0"), None);
    assert_eq!(markers(b"HGS_SESSION=\0HGS_RUN_ID=run-1\0"), None);
    assert_eq!(markers(b""), None);
}

#[test]
fn the_ancestor_walk_starts_at_this_command_and_is_bounded() {
    // The test runner itself can have a protected same-user ancestor; that is
    // a refusal, never a truncated successful walk. Topology is tested below.
    if let Ok(chain) = ancestors() {
        assert_eq!(chain.first().map(|row| row.pid), Some(std::process::id()));
        assert!(chain.len() <= 64);
    }
}

/// The child that reports what the proof decides. It does nothing unless the
/// parent asks for it, so an ordinary test run never prints.
#[test]
fn probe() {
    if std::env::var("HGS_TEAMS_CALLER_PROBE").as_deref() != Ok("1") {
        return;
    }
    #[cfg(unix)]
    println!("PARENT {}", std::os::unix::process::parent_id());
    let config = crate::config::Config::load().expect("the probe's configuration");
    if let Ok(words) = std::env::var("HGS_TEAMS_DISPATCH_PROBE") {
        let args = words
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let exit = crate::teams::dispatch(&config, &args, false).unwrap();
        println!("DISPATCH {exit}");
        println!(
            "READS {}",
            crate::state::session_graph::caller_binding_reads()
        );
        return;
    }
    if std::env::var_os("HGS_TEAMS_PANIC_PROBE").is_some() {
        let before: std::collections::BTreeMap<_, _> = std::env::vars_os().collect();
        let result = std::panic::catch_unwind(|| {
            prove_read(
                &config,
                "borrowed",
                "borrowed-run",
                None,
                |name, run, _session| {
                    assert_eq!((name, run), ("borrowed", "borrowed-run"));
                    panic!("injected explicit binding read panic");
                },
            )
        });
        assert!(result.is_err());
        assert_eq!(before, std::env::vars_os().collect());
        println!("PANIC_ENV_UNCHANGED");
    }
    if std::env::var_os("HGS_TEAMS_BIRTH_PROBE").is_some() {
        birth_probe();
    }
    // Read the native process-only walk as well: the environment-reading
    // walk below refuses before producing a chain when a middle is withheld.
    if let Ok(chain) = ancestry::processes() {
        for row in chain {
            println!(
                "ENVIRONMENT pid={} top={} readable={}",
                row.pid,
                row.top,
                environment(row.pid).is_ok()
            );
        }
    }
    if let Ok(chain) = ancestors() {
        for row in chain {
            println!(
                "ANCESTOR pid={} parent={} top={} environment_readable={}",
                row.pid,
                row.identity.parent,
                row.top,
                environment(row.pid).is_ok()
            );
        }
    }
    let answer = match caller(&config) {
        Caller::Person => "person".to_owned(),
        Caller::Session(key) => format!("session:{}", key.session_id),
        Caller::Refused(problem) => {
            println!("DETAIL {}", problem.message);
            format!("refused:{}", problem.code)
        }
    };
    println!("PROBE {answer}");
    println!(
        "READS {}",
        crate::state::session_graph::caller_binding_reads()
    );
    println!(
        "HOST_READS {}",
        crate::state::session_graph::caller_host_reads()
    );
    if let Ok(action) = std::env::var("HGS_TEAMS_PREVIEW_PROBE") {
        let exit = crate::teams::dispatch(&config, &["preview".into(), action], false).unwrap();
        println!("PREVIEW {exit}");
    }
}

/// The record check against the birth the walk read from each claimed
/// ancestor, then against that birth one unit later. The live agent must
/// match only the birth the walk itself read, through the same interface.
fn birth_probe() {
    let chain = ancestors().expect("the walk reaches the claimed agent");
    for row in chain {
        let (Some(claim), Some(birth)) = (&row.claim, &row.identity.birth) else {
            continue;
        };
        let answer = |birth: &str| match crate::state::session_graph::caller_agent_for(
            &claim.name,
            &claim.run,
            claim.session.as_deref(),
            row.pid,
            birth,
        ) {
            Ok(Some(_)) => "agent",
            Ok(None) => "none",
            Err(_) => "refused",
        };
        println!(
            "BIRTH pid={} birth={birth} walk={} later={}",
            row.pid,
            answer(birth),
            answer(&later_birth(birth))
        );
    }
}

/// The same birth one unit later: a clock tick on Linux, a microsecond on
/// macOS.
fn later_birth(birth: &str) -> String {
    let (head, unit) = birth.rsplit_once(':').unwrap_or(("", birth));
    let unit = unit.parse::<u64>().expect("a numeric birth") + 1;
    if head.is_empty() {
        unit.to_string()
    } else {
        format!("{head}:{unit}")
    }
}

/// The probe's native environment read, including when the caller's walk
/// refused below the top. Missing evidence is a fixture failure.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn environment_readable(text: &str, pid: u32) -> bool {
    let prefix = format!("ENVIRONMENT pid={pid} ");
    let row = text
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no environment evidence for {pid}: {text}"));
    match row.rsplit_once(" readable=").map(|(_, value)| value) {
        Some("true") => true,
        Some("false") => false,
        _ => panic!("invalid environment evidence for {pid}: {text}"),
    }
}

// The macOS parser and the claim it feeds, on every platform.
mod environment;

// The match against recorded resident agent hosts, on every platform.
mod hosts;

// A marked DeepSeek claim takes the hosted proof only (zerus-63rru.28).
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod hosted;

// Any other marked claim below a recorded host is refused (zerus-63rru.28).
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod under_host;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn catalog(config: &std::path::Path) {
    std::fs::create_dir_all(config.join("swarm")).unwrap();
    let value = json!({"node_id":"00000000-0000-4000-8000-0000000000a1",
        "catalog":{"schema":1,"swarm_id":"00000000-0000-4000-8000-0000000000a2","operations":{}}});
    std::fs::write(config.join("swarm/catalog.json"), value.to_string()).unwrap();
}

// The real-process suites run on Linux and macOS. Cases that need Linux
// facilities stay Linux-only inside them: user, PID and mount namespaces, a
// nondumpable process, and an environment over the 1 MiB cap, which macOS
// cannot start because its argument and environment limit is 1 MiB.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod regressions;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod process {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    const NAME: &str = "claude/example/probe";
    const RUN: &str = "run-probe";

    /// The live process that carries the markers.
    enum Agent {
        /// Python, whose environment every supported platform shows.
        Visible,
        /// `/bin/sh`, whose environment some macOS hosts withhold.
        Shell,
    }

    /// A visible agent: writes its process ID, waits for the flag, then runs
    /// the probe below it, with or without the markers.
    const VISIBLE: &str = "import os,sys,time,subprocess
pid,flag,out,strip,exe=sys.argv[1:6]
open(pid,'w').write(str(os.getpid()))
while not os.path.exists(flag): time.sleep(.01)
env=dict(os.environ)
if strip=='1':
    for key in ('HGS_SESSION','HGS_RUN_ID','HGS_SESSION_ID'): env.pop(key,None)
with open(out,'w') as sink:
    subprocess.run([exe,'--exact','teams::caller::tests::probe','--nocapture'],env=env,stdout=sink,stderr=subprocess.STDOUT)
";

    /// One detached command tree: its state directory, the live process that
    /// carries the markers, and the output of the `hgs team`-side proof below
    /// it.
    struct Probe {
        _temp: tempfile::TempDir,
        state: PathBuf,
        pid: u32,
        output: PathBuf,
        flag: PathBuf,
        markers_kept: bool,
    }

    impl Probe {
        /// Starts the detached tree with the markers its own environment holds,
        /// and strips them from the command below it unless `markers_kept`.
        fn start(markers_kept: bool, agent: Agent) -> Self {
            let temp = tempfile::tempdir().expect("a temporary folder");
            let state = temp.path().join("state");
            let config = temp.path().join("config");
            std::fs::create_dir_all(&state).unwrap();
            std::fs::create_dir_all(&config).unwrap();
            let state = std::fs::canonicalize(state).unwrap();
            let config = std::fs::canonicalize(config).unwrap();
            catalog(&config);
            let pid_file = temp.path().join("pid");
            let flag = temp.path().join("go");
            let output = temp.path().join("output");
            let strip = if markers_kept {
                ""
            } else {
                "env -u HGS_SESSION -u HGS_RUN_ID -u HGS_SESSION_ID "
            };
            let exe = std::env::current_exe().expect("this test binary");
            let mut command = Command::new("sh");
            match agent {
                Agent::Shell => {
                    let inner = format!(
                        "sh -c 'echo $$ > {}; while [ ! -f {} ]; do sleep 0.01; done; {}{} --exact teams::caller::tests::probe --nocapture > {} 2>&1'",
                        pid_file.display(),
                        flag.display(),
                        strip,
                        exe.display(),
                        output.display()
                    );
                    command.arg("-c").arg(format!("{inner} & exit 0"));
                }
                Agent::Visible => {
                    command
                        .args(["-c", "\"$@\" & exit 0", "fixture", "python3", "-c", VISIBLE])
                        .arg(&pid_file)
                        .arg(&flag)
                        .arg(&output)
                        .arg(if markers_kept { "0" } else { "1" })
                        .arg(&exe);
                }
            }
            let status = command
                .env("HGS_SESSION", NAME)
                .env("HGS_RUN_ID", RUN)
                .env("HGS_TEAMS_CALLER_PROBE", "1")
                .env("HGS_STATE_DIR", &state)
                .env("HGS_CONFIG_DIR", &config)
                .env_remove("HGS_SESSION_ID")
                .status()
                .expect("a shell");
            assert!(status.success());
            wait_for(&pid_file, Duration::from_secs(5)).expect("the detached process's pid");
            let pid: u32 = std::fs::read_to_string(&pid_file)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            Self {
                _temp: temp,
                state,
                pid,
                output,
                flag,
                markers_kept,
            }
        }

        fn state(&self) -> &Path {
            &self.state
        }

        /// The saved binding that proves this process is a live session.
        fn record(&self, process_start: &str) -> PathBuf {
            let path = self
                .state
                .join(format!("{:x}.json", Sha256::digest(NAME.as_bytes())));
            let record = json!({
                "version": 1,
                "name": NAME,
                "agent": "claude",
                "run_id": RUN,
                "launch_id": "00000000-0000-4000-8000-0000000000c1",
                "session_id": "00000000-0000-4000-8000-0000000000c1",
                "conversation_id": "00000000-0000-4000-8000-0000000000c2",
                "pid": self.pid,
                "process_start": process_start,
                "state": "running",
                "pane": "%0"
            });
            std::fs::write(&path, serde_json::to_string(&record).unwrap()).unwrap();
            path
        }

        fn born(&self) -> String {
            for _ in 0..100 {
                if let Some(start) = crate::platform::process_start_time(self.pid) {
                    return start;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the detached process has no start time");
        }

        /// Lets the command below run, then reads the line the proof printed.
        fn answer(&self) -> String {
            std::fs::write(&self.flag, b"go").unwrap();
            let until = Instant::now() + Duration::from_secs(30);
            let mut text = String::new();
            while Instant::now() < until {
                text = std::fs::read_to_string(&self.output).unwrap_or_default();
                if let Some(answer) = text.lines().find_map(|line| line.strip_prefix("PROBE ")) {
                    return answer.trim().to_owned();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the probe printed no answer: {text}");
        }

        /// Everything the probe printed, after [`Probe::answer`].
        fn text(&self) -> String {
            std::fs::read_to_string(&self.output).unwrap_or_default()
        }
    }

    fn wait_for(path: &Path, limit: Duration) -> Option<()> {
        let until = Instant::now() + limit;
        while Instant::now() < until {
            if path.metadata().is_ok_and(|meta| meta.len() > 0) {
                return Some(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    /// A process that is not above this command, recorded as another session.
    fn sibling_record(state: &Path) -> std::process::Child {
        let child = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("a sibling process");
        let name = "codex/example/sibling";
        let path = state.join(format!("{:x}.json", Sha256::digest(name.as_bytes())));
        let record = json!({
            "version": 1,
            "name": name,
            "agent": "codex",
            "run_id": "run-sibling",
            "launch_id": "00000000-0000-4000-8000-0000000000c3",
            "session_id": "00000000-0000-4000-8000-0000000000c3",
            "conversation_id": "00000000-0000-4000-8000-0000000000c4",
            "pid": child.id(),
            "process_start": crate::platform::process_start_time(child.id()).unwrap_or_default(),
            "state": "running",
            "pane": "%1"
        });
        std::fs::write(&path, serde_json::to_string(&record).unwrap()).unwrap();
        child
    }

    #[test]
    fn an_unmarked_command_under_a_live_session_agent_is_refused() {
        let probe = Probe::start(false, Agent::Visible);
        assert!(!probe.markers_kept);
        probe.record(&probe.born());
        assert_eq!(probe.answer(), "refused:not_allowed");
    }

    #[test]
    fn an_unmarked_command_with_an_established_stale_agent_is_the_person() {
        let probe = Probe::start(false, Agent::Visible);
        probe.record("stale-birth");
        let mut sibling = sibling_record(probe.state());
        // The sibling is recorded and live, so a lookup by "any live record"
        // would refuse; only an ancestor may.
        assert_eq!(probe.answer(), "person");
        let _ = sibling.kill();
        let _ = sibling.wait();
    }

    #[test]
    fn a_stale_record_does_not_match_the_reused_process_id() {
        let probe = Probe::start(false, Agent::Visible);
        probe.record("0");
        assert_eq!(probe.answer(), "person");
    }

    #[test]
    fn a_marked_command_with_a_live_record_is_that_session() {
        let probe = Probe::start(true, Agent::Visible);
        probe.record(&probe.born());
        assert_eq!(
            probe.answer(),
            "session:launch_id:00000000-0000-4000-8000-0000000000c1"
        );
    }

    #[test]
    fn a_marked_command_with_no_live_record_is_refused() {
        let probe = Probe::start(true, Agent::Visible);
        assert_eq!(probe.answer(), "refused:invalid_state");
    }

    /// The stated macOS limit: an agent whose own environment the system
    /// withholds (here `/bin/sh`) and which is itself the top of the user's
    /// tree is not detected, like an agent started without markers. Where
    /// its environment is readable, the command below it is refused.
    #[test]
    fn a_withheld_agent_at_the_top_is_the_stated_macos_limit() {
        for agent in [Agent::Shell, Agent::Visible] {
            let probe = Probe::start(false, agent);
            probe.record(&probe.born());
            let answer = probe.answer();
            let text = probe.text();
            assert!(text.contains(&format!("PARENT {}", probe.pid)), "{text}");
            if !environment_readable(&text, probe.pid) {
                assert_eq!(answer, "person", "{text}");
                let top = format!("ENVIRONMENT pid={} top=true readable=false", probe.pid);
                assert!(text.contains(&top), "{text}");
            } else {
                assert_eq!(answer, "refused:not_allowed", "{text}");
            }
        }
    }
}

/// The lifted gate: macOS is available to both callers and, having no
/// namespaces, has no admission. The real-process suites decide the rest.
#[cfg(target_os = "macos")]
#[test]
fn macos_admits_both_callers_to_the_real_process_proof() {
    assert_eq!(context::available(), Ok(()));
    assert_eq!(context::admission(), Ok(()));
}

/// The child that prints what the caller proof decides inside a private
/// PID namespace (zerus-63rru.76). It does nothing unless the parent asks.
#[test]
fn isolated_probe() {
    if std::env::var("HGS_ISOLATED_PROBE").as_deref() != Ok("1") {
        return;
    }
    let config = crate::config::Config::load().expect("the probe's configuration");
    match crate::teams::caller::caller(&config) {
        crate::teams::caller::Caller::Refused(problem) => {
            println!("REFUSAL {}", problem.message)
        }
        other => println!("OTHER {other:?}"),
    }
}

/// zerus-63rru.76: a member whose shell runs inside a private PID
/// namespace (an agent's sandbox) cannot be proven; the Teams caller names
/// the separate process namespace and the remedy, never the generic
/// refusal. The real caller proof runs in the unshared child; this is the
/// process-level case the real-agent suite cannot host without a real
/// agent account.
#[test]
#[cfg(target_os = "linux")]
fn a_member_inside_a_private_pid_namespace_gets_the_named_refusal() {
    let exe = std::env::current_exe().expect("this test binary");
    let temp = tempfile::tempdir().expect("a temporary folder");
    let state = temp.path().join("state");
    let config = temp.path().join("config");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    let output = std::process::Command::new("unshare")
        .args([
            "--pid",
            "--fork",
            "--mount-proc",
            "--",
            exe.to_str().unwrap(),
            "--exact",
            "teams::caller::tests::isolated_probe",
            "--nocapture",
        ])
        .env("HGS_ISOLATED_PROBE", "1")
        .env("HGS_SESSION", "claude/sandboxed-member")
        .env("HGS_RUN_ID", "run-sandboxed")
        .env("HGS_STATE_DIR", &state)
        .env("HGS_CONFIG_DIR", &config)
        .env_remove("HGS_SESSION_ID")
        .output()
        .expect("the unshared child");
    if !output.status.success()
        && String::from_utf8_lossy(&output.stderr).contains("Operation not permitted")
    {
        // This environment cannot create a private PID namespace without
        // privileges; the real-agent suite hosts the fixture.
        return;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("separate process namespace"),
        "the refusal does not name the namespace: {text} / {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("run outside that sandbox")
            && text.contains("private process namespace turned off"),
        "the refusal does not name the remedy: {text}"
    );
}

/// The mapping's pin at the unit level (zerus-63rru.76): the isolated PID
/// namespace cause keeps its own sentence through the Teams caller's
/// refusal; it is never flattened into the generic one.
#[test]
#[cfg(unix)]
fn the_isolated_pid_namespace_cause_keeps_its_own_sentence() {
    let problem = crate::teams::caller::host_refusal(
        crate::teams::caller::Refusal::DeepSeek(
            crate::state::dsh::Refusal::IsolatedPidNamespace,
        ),
    );
    assert!(
        problem.message.contains("separate process namespace")
            && problem.message.contains("private process namespace turned off"),
        "{:?}",
        problem.message
    );
}
