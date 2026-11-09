//! One bounded native read worker; no SQLite connection, input or task writer.
use super::{authority, wire::Envelope};
use crate::{config::Config, state::task_controls::Captured, tasks::controls::wire::Command};
use anyhow::{ensure, Result};
use std::{thread, time::Instant};

pub(super) struct Proof {
    pub actor: authority::Executor,
    pub stops: Captured,
}
impl Proof {
    pub fn verify(&self, until: Instant) -> Result<()> {
        self.actor.current(until)?;
        self.stops.verify().map_err(anyhow::Error::msg)?;
        ensure!(
            self.actor.executor() == self.stops.executor(),
            "captured executor changed"
        );
        Ok(())
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
