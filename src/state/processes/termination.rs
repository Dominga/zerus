//! Stop a verified Unix agent tree before terminal teardown loses its ancestry.
//! Never infer ownership from a folder, executable name or reusable process group.
use super::*;
use crate::platform::{PinnedProcess, ProcessRow, ProcessState};
use std::process::Command;
use std::time::{Duration, Instant};

const MAX_PROCESSES: usize = 4096;

struct Frozen {
    process: PinnedProcess,
    resume_on_drop: bool,
}
impl Frozen {
    fn new(pid: u32, birth: &str, deadline: Instant) -> Result<Self> {
        let mut frozen = Self {
            process: PinnedProcess::pin(pid, birth)?,
            resume_on_drop: false,
        };
        if frozen.process.state()? == ProcessState::Running {
            frozen.resume_on_drop = true;
            frozen.process.stop()?;
        }
        while frozen.process.state()? == ProcessState::Running {
            if Instant::now() >= deadline {
                return Err("Could not suspend the session process tree".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(frozen)
    }
}
impl Drop for Frozen {
    fn drop(&mut self) {
        if self.resume_on_drop {
            let _ = self.process.resume();
        }
    }
}

pub(in crate::state) struct Termination {
    processes: BTreeMap<u32, Frozen>,
    // Restore temporary stops before restoring signal handling. SIGHUP/SIGTERM
    // cancel acquisition through its bounded loop, allowing RAII cleanup.
    _signals: crate::platform::ShutdownGuard,
}

fn process_table(deadline: Instant) -> Result<BTreeMap<u32, ProcessRow>> {
    let (status, out, _) = crate::platform::run_bounded(
        Command::new("ps")
            .args(["-axo", "pid=,ppid=,pgid="])
            .env("LC_ALL", "C"),
        [1024 * 1024, 4096],
        deadline,
        &|| crate::platform::shutdown_signal() != 0,
    )
    .map_err(|_| "Could not verify the session process tree")?;
    if !status.success() {
        return Err("Could not read the session process tree".into());
    }
    let mut rows = BTreeMap::new();
    for line in std::str::from_utf8(&out)
        .map_err(|_| "Invalid process inventory")?
        .lines()
    {
        let numbers = line
            .split_whitespace()
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| "Invalid process inventory")?;
        if numbers.len() != 3 || numbers[0] == 0 || rows.len() >= 32768 {
            return Err("Incomplete or excessive process inventory".into());
        }
        let row = ProcessRow {
            pid: numbers[0],
            parent: numbers[1],
            group: Some(numbers[2]),
        };
        if rows.insert(row.pid, row).is_some() {
            return Err("Duplicate process identity in inventory".into());
        }
    }
    if !rows.contains_key(&std::process::id()) {
        return Err("Current process is absent from the inventory".into());
    }
    Ok(rows)
}

impl Termination {
    pub(in crate::state) fn prepare(record: &Value) -> Result<Option<Self>> {
        let root = record["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid > 0)
            .ok_or("Invalid agent PID")?;
        if !process_alive(record) {
            return if crate::platform::process_state(root, None)? == ProcessState::Exited {
                Ok(None)
            } else {
                Err("Agent process identity changed; terminal kept".into())
            };
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        let signals = crate::platform::ShutdownGuard::install().map_err(|e| e.to_string())?;
        let rows = process_table(deadline)?;
        // An invocation inside this tree cannot both kill itself and confirm the
        // result. Refuse before suspending any caller or caller ancestor.
        let mut caller = std::process::id();
        for _ in 0..=rows.len() {
            if caller == root {
                return Err("Terminate this session from outside its own process tree".into());
            }
            let Some(row) = rows.get(&caller) else {
                break;
            };
            if row.parent <= 1 || row.parent == caller {
                break;
            }
            caller = row.parent;
        }
        let identity = crate::platform::process_identity(root)
            .ok_or("Agent process identity is unavailable")?;
        if !process_alive(record) {
            return Err("Agent identity changed before termination".into());
        }
        let mut owned = Self {
            processes: BTreeMap::new(),
            _signals: signals,
        };
        owned
            .processes
            .insert(root, Frozen::new(root, &identity.birth, deadline)?);
        let mut stable = false;
        loop {
            if Instant::now() >= deadline || crate::platform::shutdown_signal() != 0 {
                return Err("Session process verification was interrupted; terminal kept".into());
            }
            if owned.processes[&root].process.state()? != ProcessState::Stopped {
                return Err("Agent exited during process verification; terminal kept".into());
            }
            let rows = process_table(deadline)?;
            let mut added = false;
            // Parents are stopped before their children. Repeat within a sample
            // for PID wraparound and arbitrary tree depth; sample again afterward
            // to include children forked just before a parent became stopped.
            loop {
                let mut progress = false;
                for row in rows.values() {
                    if owned.processes.contains_key(&row.pid) {
                        continue;
                    }
                    let Some(parent) = owned.processes.get(&row.parent) else {
                        continue;
                    };
                    if parent.process.state()? != ProcessState::Stopped {
                        return Err("Session process ancestry changed; terminal kept".into());
                    }
                    let Some(identity) = crate::platform::process_identity(row.pid) else {
                        if crate::platform::process_state(row.pid, None)? == ProcessState::Exited {
                            continue;
                        }
                        return Err(
                            "A session child process could not be identified; terminal kept".into(),
                        );
                    };
                    if identity.parent != row.parent {
                        continue;
                    }
                    if owned.processes.len() >= MAX_PROCESSES || Instant::now() >= deadline {
                        return Err(
                            "Session process tree exceeded its verification bound; terminal kept"
                                .into(),
                        );
                    }
                    let child = Frozen::new(row.pid, &identity.birth, deadline)?;
                    if owned.processes[&row.parent].process.state()? != ProcessState::Stopped {
                        return Err("Session process ancestry changed; terminal kept".into());
                    }
                    owned.processes.insert(row.pid, child);
                    progress = true;
                    added = true;
                }
                if !progress {
                    break;
                }
            }
            if !added && stable {
                for frozen in owned.processes.values() {
                    if frozen.process.state()? == ProcessState::Running {
                        return Err("A session process resumed during verification; terminal kept".into());
                    }
                }
                if owned.processes[&root].process.state()? != ProcessState::Stopped {
                    return Err("Agent identity changed during process verification; terminal kept".into());
                }
                return Ok(Some(owned));
            }
            stable = !added;
        }
    }

    /// Called only after the exact terminal accepted teardown. Frozen children
    /// cannot fork replacements while their confirmed identities are killed.
    /// Terminate is forced; Pause and native Stop retain their existing meaning.
    pub(in crate::state) fn finish(&self) -> Result<()> {
        // End the agent too: resuming it for a hangup would allow its shutdown
        // handlers to fork after the verified tree was fenced. Reparenting also
        // lets init reap killed children while the agent's supervisor reaps it.
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut signalled = std::collections::BTreeSet::new();
        loop {
            let mut remaining = false;
            for (&pid, frozen) in &self.processes {
                // Darwin can temporarily withhold BSD info while a killed PID
                // is still present. Unknown is pending, never proof of exit.
                match frozen.process.state() {
                    Ok(ProcessState::Exited) => {}
                    Ok(_) => {
                        remaining = true;
                        if !signalled.contains(&pid) && frozen.process.kill().is_ok() {
                            signalled.insert(pid);
                        }
                    }
                    Err(_) => remaining = true,
                }
            }
            if !remaining {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Terminal closed, but owned background processes have not exited; cleanup was not started".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}
