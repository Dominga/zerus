//! This one Windows operation has a process-entry budget, including config and
//! PATH preparation. The worker never prints, kills a process or retries.
use super::{Config, Error, Result};
use crate::desktop::SWARM_RESTART_ACQUIRING as ACQUIRING;
use serde_json::{json, Value};
use std::{
    env,
    sync::{atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(2);
pub(super) struct Request {
    target: String,
    dry: bool,
    expected: Option<String>,
    invalid_client: bool,
    invalid_guard: bool,
}

/// Recognize only the desktop operation, never these words in agent argv.
/// The existing leading selectors retain their order and last-value behavior.
pub(super) fn select(args: &[String]) -> Option<Request> {
    let (mut index, mut target, mut dry, mut client) =
        (0, String::new(), false, "local".to_owned());
    let mut invalid_client = false;
    while let Some(arg) = args.get(index) {
        if let Some(host) = arg.strip_prefix('@') {
            target = host.into();
            index += 1;
        } else if arg == "--dry-run" {
            dry = true;
            index += 1;
        } else if arg == "--client" {
            client = args.get(index + 1)?.clone();
            invalid_client |= client.is_empty();
            index += 2;
        } else {
            break;
        }
    }
    let mut expected = None;
    let mut invalid_guard = false;
    if args.get(index).is_some_and(|a| a == "--expect-machine") {
        expected = Some(args.get(index + 1)?.clone());
        index += 2;
        if args.get(index).is_some_and(|a| a == "--client") {
            // A late guarded call already carries its nondefault --client.
            invalid_guard = client != "local";
            invalid_client |= args.get(index + 1)?.is_empty();
            index += 2;
        }
    }
    (args[index..] == ["desktop", "swarm-restart", "--json"]).then_some(Request {
        target,
        dry,
        expected,
        invalid_client,
        invalid_guard,
    })
}
impl Request {
    fn validate(&self) -> Result<()> {
        if self.invalid_client {
            return Err(Error::new(1, "--client needs a peer name"));
        }
        if self.invalid_guard {
            return Err(Error::new(
                2,
                "A guarded mobile call requires a local command",
            ));
        }
        if self.dry {
            return Err(Error::new(
                2,
                "desktop manages only this computer; remote desktop IPC is refused",
            ));
        }
        if let Some(expected) = &self.expected {
            if uuid::Uuid::parse_str(expected)
                .ok()
                .is_none_or(|id| id.to_string() != *expected)
            {
                return Err(Error::new(
                    1,
                    "Mobile target identity changed; refresh computers",
                ));
            }
        }
        // A config-defined local alias is checked after config load, on the
        // worker. An explicit nonempty environment override needs no I/O.
        if !self.target.is_empty()
            && env::var("HGS_SELF")
                .ok()
                .filter(|s| !s.is_empty())
                .is_some_and(|names| !names.split_whitespace().any(|name| name == self.target))
        {
            return Err(Error::new(
                2,
                "desktop manages only this computer; remote desktop IPC is refused",
            ));
        }
        Ok(())
    }
    fn run(self, until: Instant) -> Result<Value> {
        crate::platform::prepare_environment();
        let config = Config::load()?;
        check_deadline(until)?;
        if !self.target.is_empty() && !config.is_self(&self.target) {
            return Err(Error::new(
                2,
                "desktop manages only this computer; remote desktop IPC is refused",
            ));
        }
        if let Some(expected) = self.expected {
            let actual =
                crate::swarm::local_node_id(&config).map_err(|e| Error::new(1, e.to_string()))?;
            if actual != expected {
                return Err(Error::new(
                    1,
                    "Mobile target identity changed; refresh computers",
                ));
            }
        }
        check_deadline(until)?;
        #[cfg(windows)]
        {
            crate::desktop::restart_swarm_until(&config, until)
                .map_err(|e| Error::new(2, e.to_string()))
        }
        #[cfg(not(windows))]
        {
            Err(Error::new(2, "Windows desktop restart is unavailable"))
        }
    }
}
fn check_deadline(until: Instant) -> Result<()> {
    if Instant::now() >= until {
        Err(Error::new(
            3,
            "desktop swarm restart outcome is unconfirmed; do not repeat this attempt",
        ))
    } else {
        Ok(())
    }
}
fn unknown() -> Result<i32> {
    println!(
        "{}",
        json!({"schema":1,"accepted":false,"outcome":"unknown","code":"swarm_restart_unconfirmed","retry_allowed":false})
    );
    // Main exits directly; its ordinary error path cannot perform more work.
    Ok(3)
}
pub(super) fn dispatch(request: Request, entered: Instant) -> Result<i32> {
    request.validate()?;
    let wait = WAIT;
    #[cfg(test)]
    let wait = if env::var("HGS_SWARM_ENTRY_PROBE").ok().as_deref() == Some("owned") {
        Duration::from_millis(250)
    } else {
        wait
    };
    let until = entered + wait;
    if Instant::now() >= until
        || ACQUIRING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return unknown();
    }
    let (send, receive) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("swarm-cli-entry".into())
        .spawn(move || {
            struct Admission;
            impl Drop for Admission {
                fn drop(&mut self) {
                    ACQUIRING.store(false, Ordering::Release);
                }
            }
            let _admission = Admission;
            let result = request.run(until);
            let _ = send.try_send(result);
        });
    if let Err(error) = worker {
        ACQUIRING.store(false, Ordering::Release);
        return Err(error.into());
    }
    match receive.recv_timeout(until.saturating_duration_since(Instant::now())) {
        Ok(result) if Instant::now() < until => {
            let value = result?;
            println!("{value}");
            Ok(0)
        }
        _ => unknown(),
    }
}

#[cfg(test)]
mod tests;
