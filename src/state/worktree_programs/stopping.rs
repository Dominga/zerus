//! The original native-confirmed own helper. No actor execution, alternate
//! image, default CLI selection, automatic escalation or receipt replay.
use super::{completed, journal, *};
use crate::platform::worktree_membership::objects::loan::borrower::Borrowed;
use crate::{
    desktop::program_stop,
    platform::{PinnedProcess, ProcessState},
};
use std::{io::Write, os::fd::AsRawFd};
use zerus_client_core::{
    desktop::ui_requests::{Envelope, Request},
    worktree_programs::{
        custody::FRAME_BYTES,
        invocation::Intent,
        stop::{Input, Kind, Member, Observation, Report, State},
    },
};

fn fail(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn live(until: Instant) -> Result<()> {
    if Instant::now() >= until || crate::platform::shutdown_signal() != 0 {
        Err(INVALID.into())
    } else {
        Ok(())
    }
}
/// Bounded setup read, not a blocking read_to_end dependent on an untrusted
/// pipe's EOF. Input must be one closed envelope followed by physical EOF.
pub(super) fn read(until: Instant) -> Result<Vec<u8>> {
    let fd = io::stdin().as_raw_fd();
    let mut bytes = Vec::new();
    loop {
        live(until)?;
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = until
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(20) as i32;
        let n = unsafe { libc::poll(&mut poll, 1, millis) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(fail(e));
        }
        if n == 0 {
            continue;
        }
        if poll.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(INVALID.into());
        }
        let mut buffer = [0u8; 8192];
        let n = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if n < 0 {
            return Err(fail(io::Error::last_os_error()));
        }
        if n == 0 {
            return Ok(bytes);
        }
        if bytes.len() + n as usize > FRAME_BYTES {
            return Err(INVALID.into());
        }
        bytes.extend_from_slice(&buffer[..n as usize]);
    }
}

pub(super) fn run(args: &[String]) -> Result<i32> {
    if args != ["--json"] {
        return Err(
            "usage: hgs worktrees program-stop --json (original native envelope on stdin)".into(),
        );
    }
    let _signals = crate::platform::ShutdownGuard::install().map_err(fail)?;
    let bytes = read(Instant::now() + Duration::from_secs(5))?;
    let envelope: Envelope = serde_json::from_slice(&bytes).map_err(fail)?;
    if !envelope.valid() {
        return Err(INVALID.into());
    }
    let Request::WorktreeProgramStop { intent } = &envelope.request else {
        return Err(INVALID.into());
    };
    let config = crate::config::Config::load().map_err(fail)?;
    let value = serde_json::to_value(&envelope).map_err(fail)?;
    let report = execute(&config, intent, &value)?;
    let bytes = serde_json::to_vec(&report).map_err(fail)?;
    if bytes.len() > wire::MAX_BYTES {
        return Err(INVALID.into());
    }
    io::stdout()
        .write_all(&bytes)
        .and_then(|_| io::stdout().write_all(b"\n"))
        .map_err(fail)?;
    Ok(0)
}
/// Exact original-ID DATA lookup. A historical result cannot construct a
/// Claim, proof, borrower, phase or an executable continuation.
pub(super) fn status(args: &[String]) -> Result<Value> {
    if args != ["--json"] {
        return Err(
            "usage: hgs worktrees program-stop-status --json (original input on stdin)".into(),
        );
    }
    let _signals = crate::platform::ShutdownGuard::install().map_err(fail)?;
    let input = Input::decode(&read(Instant::now() + Duration::from_secs(5))?).map_err(fail)?;
    let config = crate::config::Config::load().map_err(fail)?;
    if super::super::session_action::status_machine(&config)? != input.manifest.capture.machine
        || zerus_desktop_migration::configuration(&config.state, &config.dir).map_err(fail)?
            != input.manifest.configuration
    {
        return Err(INVALID.into());
    }
    let Some(store) = journal::Store::open(&config.state, false)? else {
        return Ok(json!({"found":false}));
    };
    let Some((original, report)) = store.lookup(&input.request_id)? else {
        return Ok(json!({"found":false}));
    };
    if original != input {
        return Err(INVALID.into());
    }
    Ok(json!({"found":true,"report":report}))
}

fn report(input: &Input, state: State, members: Vec<Member>) -> Report {
    Report {
        request_id: input.request_id.clone(),
        binding: input.binding(),
        state,
        members,
    }
}

fn scope(config: &crate::config::Config, intent: &Intent, loan: &Borrowed) -> Result<()> {
    live(loan.deadline())?;
    let manifest = &intent.input.manifest;
    if !intent.valid()
        || !manifest.valid(now() as u64)
        || super::boot()? != manifest.boot
        || super::super::session_action::status_machine(config)? != manifest.capture.machine
        || zerus_desktop_migration::configuration(&config.state, &config.dir).map_err(fail)?
            != manifest.configuration
        || loan.scope().identity() != &manifest.checkout
    {
        return Err(INVALID.into());
    }
    loan.current(intent).map_err(fail)?;
    if loan
        .scope()
        .protects(
            &[
                &config.dir,
                &config.state,
                &PathBuf::from(&manifest.capture.common_dir),
                &std::env::current_dir().map_err(fail)?,
            ],
            loan.deadline(),
        )
        .map_err(fail)?
    {
        return Err("A program needed to settle this action uses the working folder".into());
    }
    super::super::worktree_cleanup::program_stop_check(&manifest.capture, loan.deadline())?;
    live(loan.deadline())
}
fn execute(config: &crate::config::Config, intent: &Intent, envelope: &Value) -> Result<Report> {
    if !intent.valid() {
        return Err(INVALID.into());
    }
    let loan = Borrowed::acquire(intent, Instant::now() + Duration::from_secs(15)).map_err(fail)?;
    scope(config, intent, &loan)
        .map_err(|error| format!("Original program-stop scope unavailable: {error}"))?;
    let completion = completed::observe(&config.state, &intent.input, loan.deadline())
        .map_err(|error| format!("Original Terminate completion unavailable: {error}"))?;
    let rows = loan.programs(intent).map_err(fail)?;
    originals(&intent.input, &rows)?;
    // All slow preparation precedes this original two-second canonical proof.
    // Fsync cannot renew it: consume rechecks the same proof after reservation.
    let proof = program_stop::Proof::capture(config, envelope).map_err(fail)?;
    let store = journal::Store::open(&config.state, true)?.ok_or(INVALID)?;
    let claim = match store.reserve((*intent.input).clone(), now() as u64)? {
        journal::Reservation::Existing(report) => return Ok(report),
        journal::Reservation::Fresh(claim) => claim,
    };
    let permit = match proof.consume(intent, loan.deadline(), &claim) {
        Ok(permit) => permit,
        Err(_) => return claim.finish(report(&intent.input, State::Refused, vec![])),
    };
    let mut attempted = false;
    let result = drive(
        &intent.input,
        loan.scope(),
        loan.deadline(),
        &mut attempted,
        &mut || {
            // Slow checks FIRST. The final per-process pin/membership and proof
            // checks are adjacent to the single signal in drive().
            let guards = super::super::worktree_cleanup::program_stop_guards(
                &intent.input.manifest.capture,
                loan.deadline(),
            )?;
            scope(config, intent, &loan)?;
            completion.recheck(&config.state, loan.deadline())?;
            let rows = loan.programs(intent).map_err(fail)?;
            originals(&intent.input, &rows)?;
            permit.current().map_err(fail)?;
            Ok(guards)
        },
        &mut || permit.current().map_err(fail),
    );
    let result = result.and_then(|report| {
        // Also recheck the retained source/objects for a zero-target or fully
        // exited list. Complete remains historical DATA, never cleanup consent.
        permit.current().map_err(fail)?;
        loan.current(intent).map_err(fail)?;
        Ok(report)
    });
    drop(permit);
    claim.finish(terminal(&intent.input, result, attempted))
}
fn terminal(input: &Input, result: Result<Report>, attempted: bool) -> Report {
    match result {
        Ok(report) => report,
        Err(_) => report(
            input,
            if attempted {
                State::Unknown
            } else {
                State::Refused
            },
            vec![],
        ),
    }
}
fn originals(input: &Input, rows: &[wire::Program]) -> Result<()> {
    if rows.iter().any(|now| {
        !input
            .manifest
            .programs
            .iter()
            .any(|old| old.same_process(now))
    }) {
        return Err("New or replaced programs need a fresh review".into());
    }
    Ok(())
}

struct Original {
    program: wire::Program,
    pin: Option<PinnedProcess>,
    observation: Observation,
}
fn final_state(row: &mut Original, scope: &Scope, until: Instant) -> Result<()> {
    let Some(pin) = &row.pin else {
        return Ok(());
    };
    if pin.state()? == ProcessState::Exited {
        row.observation = Observation::Exited;
        return Ok(());
    }
    row.observation =
        if worktree_membership::original_member(scope, &row.program, until).map_err(fail)? {
            Observation::Surviving
        } else {
            Observation::Outside
        };
    Ok(())
}
/// Private worker mechanism. Only execute() can reach it in production after
/// consuming the typed native proof and holding the actual original loan.
fn drive<G>(
    input: &Input,
    scope: &Scope,
    until: Instant,
    attempted: &mut bool,
    check: &mut impl FnMut() -> Result<G>,
    current: &mut impl FnMut() -> Result<()>,
) -> Result<Report> {
    let mut rows = Vec::new();
    for program in &input.manifest.programs {
        live(until)?;
        let state = crate::platform::process_state(program.pid, Some(&program.birth))?;
        let pin = if state == ProcessState::Exited {
            None
        } else {
            Some(PinnedProcess::pin(program.pid, &program.birth)?)
        };
        rows.push(Original {
            program: program.clone(),
            pin,
            observation: Observation::Exited,
        });
    }
    // Leave a bounded part of the ORIGINAL phase for final native observations
    // and the result write. This is never a second wait/deadline or a retry.
    let stop_until = until
        .checked_sub(Duration::from_millis(200))
        .ok_or(INVALID)?;
    for row in &mut rows {
        if Instant::now() >= stop_until {
            break;
        }
        let _guards = check()?;
        final_state(row, scope, until)?;
        if row.observation != Observation::Surviving {
            continue;
        }
        let pin = row.pin.as_ref().ok_or(INVALID)?;
        // Check the consumed permission again after membership I/O, never
        // authenticate/renew it. No command, PID or group is adopted here.
        current()?;
        if pin.state()? == ProcessState::Exited {
            row.observation = Observation::Exited;
            continue;
        }
        if !worktree_membership::original_member(scope, &row.program, until).map_err(fail)? {
            row.observation = Observation::Outside;
            continue;
        }
        current()?;
        live(until)?;
        *attempted = true; // A failed/interruptible syscall is not a no-effect proof.
        match &input.stop {
            Kind::Normal => pin.terminate()?,
            Kind::Force { .. } => pin.kill()?,
        }
    }
    loop {
        let mut surviving = false;
        for row in &mut rows {
            if row.observation == Observation::Surviving {
                if row.pin.as_ref().ok_or(INVALID)?.state()? == ProcessState::Exited {
                    row.observation = Observation::Exited;
                } else {
                    surviving = true;
                }
            }
        }
        if !surviving || Instant::now() >= stop_until {
            break;
        }
        live(until)?;
        std::thread::sleep(
            Duration::from_millis(5).min(stop_until.saturating_duration_since(Instant::now())),
        );
    }
    // Read-only final observations distinguish known survivors from Unknown.
    // A normal deadline never automatically escalates to SIGKILL.
    for row in &mut rows {
        final_state(row, scope, until)?;
    }
    live(until)?;
    let members: Vec<_> = rows
        .into_iter()
        .map(|row| Member {
            program: row.program,
            observation: row.observation,
        })
        .collect();
    let state = if members
        .iter()
        .any(|m| m.observation == Observation::Surviving)
    {
        State::Incomplete
    } else {
        State::Complete
    };
    let result = report(input, state, members);
    if !result.valid(input) {
        return Err(INVALID.into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, Write},
        process::{Child, Command, Stdio},
    };
    // ShutdownGuard's process-wide signal bit is intentionally sticky after
    // a stop. The production helper installs it in a fresh process. Match that
    // lifecycle here; never reset a live suite's cancellation state or lengthen
    // the native phase just because another unit test sent a synthetic signal.
    fn isolated_signal_case(leaf: &str) -> bool {
        const KEY: &str = "ZERUS_TEST_PROGRAM_STOP_BODY";
        let case = format!("state::worktree_programs::stopping::tests::{leaf}");
        if std::env::var(KEY).ok().as_deref() == Some(&case) {
            assert_eq!(crate::platform::shutdown_signal(), 0);
            return false;
        }
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([&case, "--exact", "--test-threads=1"])
            .env(KEY, &case);
        let (status, output, errors) = crate::platform::run_bounded(
            &mut child,
            [64 << 10, 64 << 10],
            Instant::now() + Duration::from_secs(10),
            &|| false,
        )
        .expect("isolated original signal fixture");
        assert!(
            status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output),
            String::from_utf8_lossy(&errors)
        );
        assert!(
            String::from_utf8_lossy(&output).contains("test result: ok. 1 passed"),
            "the exact native fixture must run once"
        );
        true
    }
    struct Process {
        child: Child,
        output: io::BufReader<std::process::ChildStdout>,
        root: tempfile::TempDir,
        marker: PathBuf,
    }
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    impl Process {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let marker = root.path().join("signal");
            std::fs::create_dir(root.path().join("work")).unwrap();
            let mut child = Command::new("python3")
                .args([
                    "-I",
                    "-c",
                    r#"import os,signal,sys
marker=sys.argv[1]
def term(*_):
 with open(marker,'ab') as f: f.write(b'T')
signal.signal(signal.SIGTERM,term)
print('ready',flush=True)
for line in sys.stdin:
 if line.strip()=='outside': os.chdir('/')
 print('ready',flush=True)
"#,
                ])
                .arg(&marker)
                .current_dir(root.path().join("work"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let output = io::BufReader::new(child.stdout.take().unwrap());
            let mut result = Self {
                child,
                output,
                root,
                marker,
            };
            result.ready();
            result
        }
        fn ready(&mut self) {
            use std::os::fd::AsRawFd;
            let mut p = libc::pollfd {
                fd: self.output.get_ref().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            assert_eq!(unsafe { libc::poll(&mut p, 1, 5000) }, 1);
            let mut line = String::new();
            self.output.read_line(&mut line).unwrap();
            assert_eq!(line, "ready\n");
        }
        fn data(&self) -> (Scope, Input) {
            let until = Instant::now() + Duration::from_secs(3);
            let root = self.root.path().join("work").canonicalize().unwrap();
            let scope = Scope::open(&root, until).unwrap();
            let capture = Capture {
                target: zerus_client_core::Target {
                    computer: String::new(),
                    name: "codex/example/original".into(),
                    run_id: "original-run".into(),
                    conversation_id: "original-conversation".into(),
                },
                machine: uuid::Uuid::new_v4().to_string(),
                root: root.to_str().unwrap().into(),
                common_dir: "/srv/example/main/.git".into(),
                owner_id: uuid::Uuid::new_v4().to_string(),
            };
            let original = String::from_utf8(
                capture
                    .invocation(&uuid::Uuid::new_v4().to_string())
                    .unwrap()
                    .stdin
                    .unwrap(),
            )
            .unwrap();
            let issued = now() as u64;
            let input = Input {
                request_id: uuid::Uuid::new_v4().to_string(),
                confirmation_id: uuid::Uuid::new_v4().to_string(),
                source: zerus_client_core::worktree_programs::stop::Source::Service {
                    epoch: uuid::Uuid::new_v4().to_string(),
                },
                archive_id: uuid::Uuid::new_v4().to_string(),
                original,
                stop: Kind::Normal,
                manifest: wire::Manifest {
                    abi: wire::ABI.into(),
                    id: uuid::Uuid::new_v4().to_string(),
                    capture,
                    configuration: "a".repeat(64),
                    boot: uuid::Uuid::new_v4().to_string(),
                    checkout: scope.identity().clone(),
                    issued,
                    expires: issued + 300,
                    programs: vec![wire::Program {
                        pid: self.child.id(),
                        birth: crate::platform::process_identity(self.child.id())
                            .unwrap()
                            .birth,
                        label: "original fixture".into(),
                    }],
                },
            };
            assert!(input.valid());
            (scope, input)
        }
    }
    #[test]
    fn normal_stop_never_escalates_and_force_is_a_new_original_with_the_same_process_pin() {
        if isolated_signal_case(
            "normal_stop_never_escalates_and_force_is_a_new_original_with_the_same_process_pin",
        ) {
            return;
        }
        // Real signal worker mechanism, synthetic checks. This does NOT claim
        // a canonical native confirmation or end-to-end helper authentication.
        let mut process = Process::new();
        let (scope, input) = process.data();
        let mut attempted = false;
        let normal = drive(
            &input,
            &scope,
            Instant::now() + Duration::from_millis(600),
            &mut attempted,
            &mut || Ok(()),
            &mut || Ok(()),
        )
        .unwrap();
        assert!(attempted);
        assert_eq!(normal.state, State::Incomplete);
        assert_eq!(normal.members[0].observation, Observation::Surviving);
        assert_eq!(std::fs::read(&process.marker).unwrap(), b"T");
        assert!(
            process.child.try_wait().unwrap().is_none(),
            "normal stop may not kill a survivor"
        );
        let mut force = input.clone();
        force.request_id = uuid::Uuid::new_v4().to_string();
        force.confirmation_id = uuid::Uuid::new_v4().to_string();
        force.manifest.id = uuid::Uuid::new_v4().to_string();
        force.stop = Kind::Force {
            previous_request_id: input.request_id.clone(),
            previous_binding: input.binding(),
        };
        assert!(force.follows(&input, &normal));
        let (forced, status) = std::thread::scope(|threads| {
            // Act as the original fixture's parent while the worker observes
            // exit. Darwin cannot read BSD metadata for an unreaped zombie;
            // that uncertainty must still refuse in production.
            let child = &mut process.child;
            let input_pipe = child.stdin.take();
            let reaped = threads.spawn(move || child.wait());
            let forced = drive(
                &force,
                &scope,
                Instant::now() + Duration::from_secs(1),
                &mut false,
                &mut || Ok(()),
                &mut || Ok(()),
            );
            // On refusal, EOF also settles this private child before joining
            // its waiter. A successful Force must still prove SIGKILL below.
            drop(input_pipe);
            (forced, reaped.join().unwrap().unwrap())
        });
        let forced = forced.unwrap();
        assert_eq!(forced.state, State::Complete);
        assert_eq!(forced.members[0].observation, Observation::Exited);
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGKILL));
    }
    #[test]
    fn outside_replaced_cancelled_or_changed_originals_never_receive_a_signal() {
        if isolated_signal_case(
            "outside_replaced_cancelled_or_changed_originals_never_receive_a_signal",
        ) {
            return;
        }
        for case in 0..4 {
            let mut process = Process::new();
            let (scope, mut input) = process.data();
            if case == 0 {
                writeln!(process.child.stdin.as_mut().unwrap(), "outside").unwrap();
                process.ready();
            }
            if case == 1 {
                input.manifest.programs[0].birth = "another-original".into();
            }
            let mut attempted = false;
            let result = drive(
                &input,
                &scope,
                Instant::now() + Duration::from_secs(1),
                &mut attempted,
                &mut || {
                    if case == 2 || case == 3 {
                        Err("original source cancelled or changed".into())
                    } else {
                        Ok(())
                    }
                },
                &mut || Ok(()),
            );
            if case >= 2 {
                assert!(result.is_err());
            } else {
                let row = result.unwrap();
                assert_eq!(row.state, State::Complete);
                assert_eq!(
                    row.members[0].observation,
                    if case == 0 {
                        Observation::Outside
                    } else {
                        Observation::Exited
                    }
                );
            }
            assert!(!attempted);
            assert!(!process.marker.exists());
            assert!(process.child.try_wait().unwrap().is_none());
        }
    }
    #[test]
    fn leaving_between_the_two_membership_checks_receives_no_signal() {
        if isolated_signal_case("leaving_between_the_two_membership_checks_receives_no_signal") {
            return;
        }
        let mut process = Process::new();
        let (scope, input) = process.data();
        let until = Instant::now() + Duration::from_secs(3);
        assert!(
            worktree_membership::original_member(&scope, &input.manifest.programs[0], until)
                .unwrap()
        );
        let mut checks = 0;
        let mut attempted = false;
        let result = drive(
            &input,
            &scope,
            until,
            &mut attempted,
            &mut || Ok(()),
            &mut || {
                checks += 1;
                if checks == 1 {
                    // This existing consumed-proof check follows final_state's
                    // real membership read and precedes the second read. Move
                    // the actual original and wait for its native response.
                    writeln!(process.child.stdin.as_mut().unwrap(), "outside").unwrap();
                    process.ready();
                }
                Ok(())
            },
        )
        .unwrap();
        assert!(!attempted);
        assert_eq!(result.state, State::Complete);
        assert_eq!(result.members[0].observation, Observation::Outside);
        assert!(!process.marker.exists());
        assert!(process.child.try_wait().unwrap().is_none());
        assert_eq!(checks, 1);
    }
    #[test]
    fn a_lost_result_after_the_first_signal_is_unknown_and_never_a_no_effect_refusal() {
        if isolated_signal_case(
            "a_lost_result_after_the_first_signal_is_unknown_and_never_a_no_effect_refusal",
        ) {
            return;
        }
        let first = Process::new();
        let second = Process::new();
        let (process, outside) = if first.child.id() < second.child.id() {
            (first, second)
        } else {
            (second, first)
        };
        let (scope, mut input) = process.data();
        input.manifest.programs.push(wire::Program {
            pid: outside.child.id(),
            birth: crate::platform::process_identity(outside.child.id())
                .unwrap()
                .birth,
            label: "second original fixture".into(),
        });
        input.manifest.programs.sort_by_key(|p| p.pid);
        let mut calls = 0;
        let mut attempted = false;
        let result = drive(
            &input,
            &scope,
            Instant::now() + Duration::from_secs(1),
            &mut attempted,
            &mut || {
                calls += 1;
                if calls >= 2 {
                    Err("source lost after the original signal".into())
                } else {
                    Ok(())
                }
            },
            &mut || Ok(()),
        );
        assert!(result.is_err());
        assert!(attempted);
        let result = terminal(&input, result, attempted);
        assert_eq!(result.state, State::Unknown);
        let (_state, claim) = journal::Claim::fixture(input.clone());
        assert_eq!(claim.finish(result).unwrap().state, State::Unknown);
        assert_eq!(
            terminal(&input, Err("before any signal".into()), false).state,
            State::Refused
        );
    }

    #[test]
    fn the_final_proof_check_and_complete_original_list_are_not_optional() {
        if isolated_signal_case("the_final_proof_check_and_complete_original_list_are_not_optional")
        {
            return;
        }
        let mut process = Process::new();
        let (scope, input) = process.data();
        let mut attempted = false;
        assert!(drive(
            &input,
            &scope,
            Instant::now() + Duration::from_secs(1),
            &mut attempted,
            &mut || Ok(()),
            &mut || Err("final original permission changed".into())
        )
        .is_err());
        assert!(!attempted);
        assert!(!process.marker.exists());
        assert!(process.child.try_wait().unwrap().is_none());
        assert!(originals(&input, &input.manifest.programs).is_ok());
        let mut changed = input.manifest.programs.clone();
        changed[0].birth = "replacement".into();
        assert!(originals(&input, &changed).is_err());
        changed[0] = wire::Program {
            pid: input.manifest.programs[0].pid + 1,
            ..changed[0].clone()
        };
        assert!(originals(&input, &changed).is_err());
        assert!(
            originals(&input, &[]).is_ok(),
            "an exited original need not remain live"
        );
    }
}
