//! Non-serializable native caller evidence. JSON role labels and environment
//! variables do not construct this object. Person has no CLI-bridge proof.
#[cfg(target_os = "linux")]
use super::peer_linux::Peer as NativePeer;
#[cfg(target_os = "linux")]
use super::peer_linux::Resident as NativeResident;
#[cfg(target_os = "macos")]
use super::peer_macos::Peer as NativePeer;
#[cfg(target_os = "macos")]
use super::peer_macos::Resident as NativeResident;
#[cfg(windows)]
use super::peer_windows::Peer as NativePeer;
#[cfg(windows)]
use super::peer_windows::Resident as NativeResident;
use crate::state::task_controls::observation::Observed;
use crate::{
    desktop::transport::Stream,
    tasks::controls::wire::{self, Command},
};
use std::{io, time::Instant};

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
struct NativePeer;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
struct NativeResident;

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Task-control executor identity is unverified",
    )
}

pub(super) struct Peer {
    native: NativePeer,
}
impl Peer {
    pub fn bind(
        mut self,
        command: &Command,
        observed: &Observed,
        until: Instant,
    ) -> io::Result<Executor> {
        if requires_person(command) {
            return Err(refused());
        }
        self.command(command, until)?;
        self.executor(
            observed.process.pid,
            &observed.process.recorded_start,
            until,
        )?;
        let resident = Resident::capture(observed, until)?;
        self.current(until)?;
        Ok(Executor {
            peer: self,
            resident,
            executor: observed.executor.clone(),
        })
    }
    pub fn capture(stream: &Stream, until: Instant) -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        let native = NativePeer::capture(stream, std::fs::File::open("/proc/self/exe")?, until)?;
        #[cfg(target_os = "macos")]
        let native = NativePeer::capture(stream, &std::env::current_exe()?, until)?;
        #[cfg(windows)]
        let native = NativePeer::capture(stream, until)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            Ok(Self { native })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (stream, until);
            Err(refused())
        }
    }
    pub fn current(&self, until: Instant) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            self.native.current(until)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = until;
            Err(refused())
        }
    }
    pub fn command(&self, command: &Command, until: Instant) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let args = self
            .native
            .arguments(until)?
            .into_iter()
            .map(String::from_utf8)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| refused())?;
        #[cfg(windows)]
        let args = self.native.arguments(until)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            actual_command(&args, command)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (command, until);
            Err(refused())
        }
    }
    /// Native agent PID/birth comes from the exact verified executor run,
    /// never request JSON or an HGS_SESSION environment label.
    pub fn executor(&mut self, pid: u32, birth: &str, until: Instant) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            self.native.executor(pid, birth, until)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (pid, birth, until);
            Err(refused())
        }
    }
}

/// No serde/Clone constructor: only the actual held peer and resident native
/// process proof create this executor. A matching Plan digest cannot do so.
pub(super) struct Executor {
    peer: Peer,
    resident: Resident,
    executor: zerus_desktop_ledger::task_controls::Executor,
}
impl Executor {
    pub fn executor(&self) -> &zerus_desktop_ledger::task_controls::Executor {
        &self.executor
    }
    pub fn current(&self, until: Instant) -> io::Result<()> {
        self.peer.current(until)?;
        self.resident.current(until)
    }
}
struct Resident {
    native: NativeResident,
}
impl Resident {
    fn capture(observed: &Observed, until: Instant) -> io::Result<Self> {
        let a = &observed.process;
        let s = &observed.supervisor;
        #[cfg(target_os = "linux")]
        let native = NativeResident::capture(
            a.pid,
            &a.recorded_start,
            s.pid,
            &s.recorded_start,
            &observed.executor.birth,
            std::fs::File::open("/proc/self/exe")?,
            until,
        )?;
        #[cfg(target_os = "macos")]
        let native = NativeResident::capture(
            a.pid,
            &a.recorded_start,
            s.pid,
            &s.recorded_start,
            &observed.executor.birth,
            &std::env::current_exe()?,
            until,
        )?;
        #[cfg(windows)]
        let native = NativeResident::capture(
            a.pid,
            &a.recorded_start,
            s.pid,
            &s.recorded_start,
            &observed.executor.birth,
            until,
        )?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let args = native
            .arguments(until)?
            .into_iter()
            .map(String::from_utf8)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| refused())?;
        #[cfg(windows)]
        let args = native.arguments(until)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            supervisor_command(&args, &observed.executor)?;
            Ok(Self { native })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (a, s, until);
            Err(refused())
        }
    }
    fn current(&self, until: Instant) -> io::Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            self.native.current(until)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = until;
            Err(refused())
        }
    }
}

fn supervisor_command(
    args: &[String],
    executor: &zerus_desktop_ledger::task_controls::Executor,
) -> io::Result<()> {
    if args.get(1).is_none_or(|s| s != "--run") {
        return Err(refused());
    }
    let raw = args.get(2).ok_or_else(refused)?;
    let count = if raw.starts_with("sid1:") {
        let parts = raw.split(':').collect::<Vec<_>>();
        if parts.len() != 4 || parts[1] != executor.session_id || parts[2] != executor.run {
            return Err(refused());
        }
        parts[3].parse::<usize>().map_err(|_| refused())?
    } else {
        raw.parse::<usize>().map_err(|_| refused())?
    };
    // At least one actual program follows the count's resume arguments. The
    // agent's identity is its pinned native child, not this program's filename.
    if count > 64 || args.get(3 + count).is_none_or(|s| s.is_empty()) {
        return Err(refused());
    }
    Ok(())
}

fn actual_command(args: &[String], expected: &Command) -> io::Result<()> {
    let args = args.get(1..).ok_or_else(refused)?;
    if args.first().is_none_or(|s| s != "task") {
        return Err(refused());
    }
    if let Command::Begin(expected) = expected {
        if args.get(1).is_none_or(|s| s != "begin-attempt") {
            return Err(refused());
        }
        let actual = wire::begin(args.get(2..).ok_or_else(refused)?).map_err(|_| refused())?;
        if actual != *expected {
            return Err(refused());
        }
    } else {
        if args.get(1).is_none_or(|s| s != "control") {
            return Err(refused());
        }
        let actual = wire::action(args.get(2..).ok_or_else(refused)?).map_err(|_| refused())?;
        if actual != expected.identity().0 {
            return Err(refused());
        }
    }
    Ok(())
}

/// There is deliberately no Person constructor. Even an authentic executor
/// cannot use a Person label, Pause/Resume or Stop release on this bridge.
pub(super) fn requires_person(command: &Command) -> bool {
    match command {
        Command::Begin(_) | Command::Status(_) => false,
        Command::Plan(p) => p.action != wire::Action::Yield,
        Command::Execute(p) => {
            matches!(p.caller, wire::Caller::Person {})
                || p.request.action != zerus_desktop_ledger::task_controls::Action::Yield
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const ID: &str = "00000000-0000-4000-8000-000000000001";
    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn actual_cli_path_and_original_begin_proof_must_both_match() {
        let command = Command::Begin(wire::Begin {
            reference: "orchard/T-234567".into(),
            attempt: ID.into(),
        });
        for actual in [
            args(&[
                "hgs",
                "task",
                "begin-attempt",
                "orchard/T-234567",
                "--attempt",
                ID,
            ]),
            args(&[
                "hgs",
                "task",
                "begin-attempt",
                "orchard/T-234567",
                "--json",
                "--attempt",
                ID,
            ]),
        ] {
            assert!(actual_command(&actual, &command).is_ok());
        }
        for actual in [
            args(&["hgs", "desktop", "request", "--json"]),
            args(&["hgs", "task", "control", "execute", "--json"]),
            args(&[
                "hgs",
                "task",
                "begin-attempt",
                "orchard/T-234567",
                "--attempt",
                "latest",
            ]),
            args(&[
                "hgs",
                "task",
                "begin-attempt",
                "orchard/T-345678",
                "--attempt",
                ID,
            ]),
        ] {
            assert!(actual_command(&actual, &command).is_err());
        }
    }
    #[test]
    fn a_status_process_cannot_offer_an_execute_envelope() {
        let status = wire::read(
            "status",
            &serde_json::to_vec(&json!({"schema":1,"target":{"scope":"request","request_id":ID}}))
                .unwrap(),
        )
        .unwrap();
        assert!(actual_command(
            &args(&["hgs", "task", "control", "status", "--json"]),
            &status
        )
        .is_ok());
        assert!(actual_command(
            &args(&["hgs", "task", "control", "execute", "--json"]),
            &status
        )
        .is_err());
    }

    #[test]
    fn the_resident_command_is_a_supervisor_and_versioned_ids_never_retarget() {
        let executor: zerus_desktop_ledger::task_controls::Executor = serde_json::from_value(json!({
            "computer":ID,"session_id":ID,"run":ID,"birth":"original-birth","conversation":null,"child":null
        })).unwrap();
        assert!(supervisor_command(&args(&["hgs", "--run", "0", "codex"]), &executor).is_ok());
        let count = format!("sid1:{ID}:{ID}:1");
        assert!(supervisor_command(
            &args(&["hgs", "--run", &count, "resume", "codex"]),
            &executor
        )
        .is_ok());
        for bad in [
            args(&["hgs", "desktop", "serve"]),
            args(&["hgs", "--run", "0"]),
            args(&["hgs", "--run", "65", "codex"]),
            args(&[
                "hgs",
                "--run",
                &format!("sid1:other-session:{ID}:0"),
                "codex",
            ]),
            args(&["hgs", "--run", &format!("sid1:{ID}:other-run:0"), "codex"]),
        ] {
            assert!(supervisor_command(&bad, &executor).is_err());
        }
    }
    #[test]
    fn a_proved_executor_still_cannot_claim_person_controls() {
        for action in [
            wire::Action::Pause,
            wire::Action::Resume,
            wire::Action::Yield,
            wire::Action::ReleaseStop,
        ] {
            let command = Command::Plan(wire::PlanRequest {
                schema: 1,
                request_id: ID.into(),
                action,
                target: wire::Target::Attempt {
                    project: "orchard".into(),
                    task: ID.into(),
                    attempt: ID.into(),
                },
                observed_pauses: vec![],
                observed_stops: vec![],
            });
            assert_eq!(requires_person(&command), action != wire::Action::Yield);
        }
    }
}
