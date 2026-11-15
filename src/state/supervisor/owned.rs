//! Dormant original supervisor program. This is live diagnostic provenance,
//! never a grace/signal/cleanup capability. Only spawn below creates an owner;
//! copied records, a generic Custody snapshot and reconnects cannot create one.
use super::*;
use sha2::{Digest, Sha256};
#[cfg(target_os = "linux")]
use std::os::unix::process::ExitStatusExt;
use std::{io, marker::PhantomData, rc::Rc, sync::Arc, time::Duration};
pub(super) mod appearance;
mod channel;
mod creation;
mod native;
pub(in crate::state) use creation::CreationSource;

pub(in crate::state) struct Original {
    #[cfg(target_os = "linux")]
    cohort: processes::custody::Custody,
    #[cfg(target_os = "macos")]
    root: Child,
    exit: Option<ExitStatus>,
    channel: channel::Channel,
    source: Arc<native::Source>,
    domain: Arc<terminating::OriginalDomain>,
    appearance: Option<Arc<appearance::Producer>>,
    binding: Value,
    startup: bool,
    unresolved: bool,
    _local: PhantomData<Rc<()>>,
}
impl super::sealed::Status for Original {}
impl ChildStatus for Original {
    fn appearance(
        &mut self,
        record: &Value,
        request: &str,
    ) -> Result<Option<appearance::Appearance>> {
        self.check();
        if !self.startup || self.unresolved || self.appearance.is_some() {
            return Err("Original startup producer is unavailable or already closed".into());
        }
        let producer = appearance::Producer::new(self, record, request)?;
        self.appearance = Some(producer.clone());
        Ok(Some(appearance::Appearance::new(producer)))
    }
    fn id(&self) -> u32 {
        #[cfg(target_os = "linux")]
        {
            self.cohort.root_id()
        }
        #[cfg(target_os = "macos")]
        {
            self.root.id()
        }
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.observe();
        if self.exit.is_some() {
            return Ok(self.exit);
        }
        #[cfg(target_os = "linux")]
        let result = (|| {
            if !self.cohort.ready_root()? {
                return Ok(None);
            }
            // Still a zombie with our retained pidfd. Never leave a reusable
            // PID in the existing shutdown forwarder after consuming status.
            crate::platform::shutdown_child_exited();
            self.cohort.consume_root().map(|code| code.map(status))
        })();
        #[cfg(target_os = "macos")]
        let result = (|| {
            if !native::root_ready(self.root.id())? {
                return Ok(None);
            }
            crate::platform::shutdown_child_exited();
            self.root.try_wait()
        })();
        match result {
            Ok(exit) => {
                self.exit = exit;
                Ok(exit)
            }
            Err(error) => {
                crate::platform::shutdown_child_exited();
                self.unresolved = true;
                Err(error)
            }
        }
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        // Only the fixed startup borrowers reach here, after their scopes and
        // original helpers physically settle. No caller callback is accepted.
        self.startup = false;
        loop {
            let exit = self.try_wait()?;
            let idle = zerus_client_core::run::writer_custody::physically_idle();
            #[cfg(target_os = "linux")]
            let settled = if idle {
                match self.cohort.poll() {
                    processes::custody::Snapshot::Complete { .. } => true,
                    processes::custody::Snapshot::Unresolved(_) => {
                        self.unresolved = true;
                        return Err(io::Error::other("original descendant custody unresolved"));
                    }
                    processes::custody::Snapshot::Pending => false,
                }
            } else {
                false
            };
            // Mac retains the actual root only. NEVER claim descendant closure.
            #[cfg(target_os = "macos")]
            let settled = idle;
            self.observe();
            if settled {
                if let Some(exit) = exit {
                    return Ok(exit);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
#[cfg(target_os = "linux")]
fn status(code: i32) -> ExitStatus {
    ExitStatus::from_raw(if code >= 0 { code << 8 } else { -code })
}
impl Original {
    fn check(&mut self) {
        // Continuity is the fixed ORIGINAL program path, not two policy
        // snapshots. There is no setter/attach API, arbitrary borrower or
        // consumer of generic Complete. Kernel/source checks can only revoke.
        if !self.source.current() || !native::wait_policy() || self.domain.current().is_err() {
            self.unresolved = true;
        }
    }
    fn observe(&mut self) {
        self.check();
        let phase = if self.unresolved {
            "unresolved"
        } else if self.startup {
            "startup"
        } else if !zerus_client_core::run::writer_custody::physically_idle() {
            "helpers_pending"
        } else {
            "original_owner"
        };
        let data = json!({"version":1,"binding":self.binding,"phase":phase,
            "writer_domain":self.domain.observation(),
            "appearance_producer":self.appearance.as_ref().map(|p|p.observation()),
            "continuity":"fixed_original_program_only",
            "root_exit":self.exit.map(|s| ExitInfo::of(s).exit_code()),
            "descendant_coverage":if cfg!(target_os="linux") {"retained_linux_cohort"} else {"unproved"},
            "effect_authority":false});
        self.channel.poll(&self.source, &self.binding, &data);
    }
}

pub(in crate::state) fn spawn(
    argv: &[String],
    name: &str,
    run_id: &str,
) -> Result<Spawned<Original>> {
    // The const-false caller is the only production route. No env/setting/CLI.
    let until = std::time::Instant::now() + Duration::from_secs(30);
    let (guard, source, mut record) = creation::begin(name, run_id, until)?;
    terminating::check_mutation(&record)?;
    if record.get("original_owner").is_some() {
        return Err("Original creation already reserved; review without replay".into());
    }
    if !native::profile() {
        return Err("Original source profile is interposed".into());
    }
    let channel = channel::Channel::new().map_err(|e| e.to_string())?;
    let binding = json!({"session":record["session_id"],"run":run_id,
        "conversation":record["conversation_id"],"expected":record["expected_id"],
        "argv_sha256":format!("{:x}",Sha256::digest(json!(argv).to_string().as_bytes())),
        "owner":std::process::id(),"owner_birth":source.birth(),
        "source":source.wire(),"endpoint":channel.path(),"nonce":uuid::Uuid::new_v4().to_string()});
    if binding.to_string().len() > 4096 {
        return Err("Original launch binding exceeds observation limit".into());
    }
    // Durable uncertainty BEFORE native creation. Any failure keeps this fence;
    // no drop, reconnect, status result or fallback clears/replays it.
    creation::reserve(&source, &guard, &mut record, &binding, until)?;
    let original = CreationSource::capture(&source, &guard, &record, until)?;
    let domain = terminating::OriginalDomain::create(&original)?;
    #[cfg(target_os = "linux")]
    let prepared = processes::custody::Custody::prepare_original().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    native::admit_root().map_err(|e| e.to_string())?;
    let mut command = crate::platform::agent_command(argv).map_err(|e| e.to_string())?;
    original.check()?;
    domain.current()?;
    let child = zerus_client_core::child_environment::spawn(&mut command)
        .map_err(|error| format!("{}: {error}", argv[0]))?;
    drop(original);
    let identity = crate::platform::process_identity(child.id());
    crate::platform::forward_shutdown(&child);
    record["pid"] = json!(child.id());
    record["process_start"] = json!(process_start(child.id()));
    record["supervisor"] =
        json!({"pid":std::process::id(),"start":process_start(std::process::id())});
    if let Some(identity) = &identity {
        super::super::process_index_writer::capture_spawn(&mut record, identity);
    } else {
        record["process_index_pin"] = json!({"version":1,"unavailable":true});
    }
    let mut binding = binding;
    binding["root"] = json!(child.id());
    binding["root_birth"] = json!(identity.as_ref().map(|i| &i.birth));
    record["original_owner"] = binding.clone();
    let owner = Original {
        #[cfg(target_os = "linux")]
        cohort: prepared.attach(child),
        #[cfg(target_os = "macos")]
        root: child,
        exit: None,
        channel,
        source: Arc::new(source),
        domain: Arc::new(domain),
        appearance: None,
        binding,
        startup: true,
        unresolved: identity.is_none(),
        _local: PhantomData,
    };
    write(&mut record)?;
    Ok(Spawned {
        child: owner,
        record,
        identity,
    })
}

#[cfg(test)]
mod tests;
