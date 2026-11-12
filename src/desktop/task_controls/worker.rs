//! One bounded native read worker; no SQLite connection, input or task writer.
use super::{authority, wire::Envelope};
use crate::{config::Config, state::task_controls::Capture, tasks::controls::wire::Command};
use anyhow::{ensure, Result};
use std::{thread, time::Instant};

pub(super) struct Proof {
    pub actor: authority::Executor,
    pub stops: Capture,
}
impl Proof {
    pub fn verify(&self, until: Instant) -> Result<()> {
        self.actor.current(until)?;
        self.stops
            .with_current(self.actor.executor(), |executor, _, check| {
                if self.actor.executor() != executor {
                    return Err("captured executor changed".into());
                }
                check()?;
                self.actor.current(until).map_err(|error| error.to_string())
            })
            .map_err(anyhow::Error::msg)
    }
}

pub(super) fn spawn(
    config: Config,
    envelope: Envelope,
    peer: authority::Peer,
    until: Instant,
) -> std::io::Result<thread::JoinHandle<(Envelope, Result<Proof>)>> {
    thread::Builder::new()
        .name("task-control-proof".into())
        .spawn(move || {
            let proof = prepare(&config, &envelope.command, peer, until);
            (envelope, proof)
        })
}
fn prepare(
    config: &Config,
    command: &Command,
    peer: authority::Peer,
    until: Instant,
) -> Result<Proof> {
    ensure!(Instant::now() < until, "task-control deadline expired");
    let Command::Execute(plan) = command else {
        anyhow::bail!("task-control effect path is unqualified")
    };
    let (observed, stops) =
        crate::state::task_controls::observe_and_capture(config, &plan.request.executor, until)
            .map_err(anyhow::Error::msg)?;
    let actor = peer.bind(command, &observed, until)?;
    let (image, loaded) = actor.guards(until)?;
    ensure!(
        image == plan.cli_image && loaded == plan.loaded_adapter,
        "task-control loaded image guard changed"
    );
    let proof = Proof { actor, stops };
    proof.verify(until)?;
    Ok(proof)
}
