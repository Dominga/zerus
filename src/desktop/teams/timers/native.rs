//! Production effects: this computer's own `hgs` as the one original child,
//! with a custody lock of its own, no inherited agent identity and a bounded
//! deadline. A child's exit is never read as a delivery; only its receipt is.
//!
//! An effect is admitted at the moment it starts: the stored facts are read
//! first, inside the doorbell's immediate team-store transaction, and the
//! service's stop flag and the preview switch are read last, in the same
//! section; a refusal there starts nothing. The spawn itself has no time
//! bound of its own, so that transaction is held from the admission until
//! the spawn returns (measured at 253 ms with a 250 ms delay before exec);
//! other writers of the team store meet their busy timeout meanwhile.
mod clear;
mod ring;
use super::ports::{Answer, ClearRequest, Effects, Result, Ring, Rung};
use crate::{
    config::Config,
    desktop::{
        original_child::{self, Codes, Ended, Input, Output, Spec, Stdin},
        teams::{
            child,
            ports::{LaunchRead, Plan},
        },
    },
    teams::store::Store,
};
use std::{
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use zerus_team_plan::model::TaskRef;

pub(in crate::desktop::teams) struct Native<'a> {
    pub config: &'a Config,
    pub machine: &'a str,
    pub stop: &'a Arc<AtomicBool>,
}
impl Effects for Native<'_> {
    fn ring(&mut self, store: &Store, ring: &Ring) -> Rung {
        ring::deliver(self, store, ring)
    }
    fn clear(&mut self, task: &TaskRef, request: &ClearRequest) -> Answer {
        clear::run(self, task, request)
    }
    fn launch(&mut self, plan: &Plan) -> Result<LaunchRead> {
        let mut native = crate::desktop::teams::native::Native {
            config: self.config.clone(),
            machine: self.machine.into(),
            stop: self.stop.clone(),
        };
        crate::desktop::teams::ports::Effects::observe(&mut native, plan)
    }
}
impl Native<'_> {
    #[cfg(test)]
    pub(in crate::desktop::teams) fn fixture_program<T>(
        path: &std::path::Path,
        run: impl FnOnce() -> T,
    ) -> T {
        struct Restore(Option<std::path::PathBuf>);
        impl Drop for Restore {
            fn drop(&mut self) {
                tests::PROGRAM.with(|slot| *slot.borrow_mut() = self.0.take());
            }
        }
        let _restore = Restore(tests::PROGRAM.with(|slot| slot.replace(Some(path.into()))));
        run()
    }

    /// Run one original with its own custody. `admit` runs right before the
    /// spawn and may hold a value, such as a store transaction, across it.
    #[allow(clippy::too_many_arguments)]
    fn original<T>(
        &self,
        id: &str,
        args: &[&str],
        input: &[u8],
        codes: Codes,
        output: Output,
        deadline: Duration,
        admit: impl FnOnce() -> std::io::Result<T>,
    ) -> Ended {
        let Ok(program) = program() else {
            return Ended::NotStarted;
        };
        let Some((held, path, identity)) = child::custody(self.config, id) else {
            return Ended::NotStarted;
        };
        let mut scoped = vec!["--expect-machine".to_owned(), self.machine.to_owned()];
        scoped.extend(args.iter().map(|a| (*a).to_owned()));
        let spec = Spec {
            custody_env: "HGS_TEAMS_CUSTODY_FD",
            input: Input::Frozen {
                stdin: Stdin::Bytes(input),
                unwritten: "team_timer_input_unwritten",
            },
            output,
            codes,
            retain: None,
        };
        let ended = original_child::run(
            &program,
            &scoped,
            spec,
            &held,
            Instant::now() + deadline,
            |cmd| {
                let admitted = admit()?;
                // The external gates last, inside the admitted section.
                if self.stop.load(Ordering::Acquire) || !crate::teams::enabled(&self.config.state) {
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                configure(cmd, self.config);
                Ok(admitted)
            },
        );
        if !matches!(ended, Ended::Unknown(_)) {
            child::remove_custody(&path, identity);
        }
        ended
    }
}
fn configure(cmd: &mut Command, config: &Config) {
    for var in [
        "HGS_SESSION",
        "HGS_SESSION_ID",
        "HGS_RUN_ID",
        "HGS_CONVERSATION_ID",
        "HGS_LAUNCH_ID",
        "HGS_LAUNCH_SESSION_ID",
        "HGS_ACCOUNT_ID",
        "HGS_PARENT",
        "TMUX",
        "TMUX_PANE",
    ] {
        cmd.env_remove(var);
    }
    cmd.env("HGS_CONFIG_DIR", &config.dir)
        .env("HGS_STATE_DIR", &config.state);
}
fn program() -> std::io::Result<std::path::PathBuf> {
    #[cfg(test)]
    if let Some(path) = tests::PROGRAM.with(|slot| slot.borrow().clone()) {
        return Ok(path);
    }
    crate::self_exec::current()
}
#[cfg(test)]
mod tests;
