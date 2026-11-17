//! Native person authority for a keyless, exact-input acknowledgment.
#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::store::Location;
use super::{fanout::Queue, ui_authority};
use crate::config::Config;
use anyhow::{ensure, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{io::Read, sync::Arc, thread::JoinHandle};
use zerus_client_core::input_acknowledgment::Acknowledgment;

enum Origin {
    Native(Box<ui_authority::Validated>),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Terminal(ui_authority::Scope),
}
/// No decoder or agent-callable state entry can construct this authority.
pub(crate) struct Person {
    origin: Origin,
    acknowledgment: Acknowledgment,
}
impl Person {
    pub(crate) fn check(&mut self, acknowledgment: &Acknowledgment) -> crate::state::Result<()> {
        let check = || -> Result<()> {
            ensure!(
                cfg!(any(target_os = "linux", target_os = "macos")),
                "Input acknowledgment is unavailable on this platform"
            );
            ensure!(
                &self.acknowledgment == acknowledgment,
                "Person decision changed"
            );
            match &self.origin {
                Origin::Native(authority) => ensure!(
                    authority.input_acknowledgment()? == acknowledgment,
                    "Native decision changed"
                ),
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                Origin::Terminal(scope) => ensure!(
                    ui_authority::registration::caller(scope)? == "interactive_terminal",
                    "Use a person terminal outside Zerus"
                ),
            }
            Ok(())
        };
        check().map_err(|e| e.to_string())
    }
    pub(crate) fn source(&self) -> &'static str {
        match &self.origin {
            Origin::Native(_) => "native_frontend",
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Origin::Terminal(_) => "interactive_terminal",
        }
    }
}

fn outcome(ack: &Acknowledgment, result: crate::state::Result<Value>) -> Value {
    result.unwrap_or_else(|error| {
        // A reserved or possibly committed decision is unknown, never a claim
        // that nothing happened. Only point lookup is allowed after an error.
        match crate::state::input_acknowledgment::lookup(ack) {
            Ok(Some(receipt)) => receipt,
            evidence => json!({"op":"input_acknowledge","request_id":ack.request_id,
                "accepted":false,"durable":false,"status":if evidence.is_ok() {"failed"} else {"unknown"},
                "code":"input_acknowledgment_refused","error":error}),
        }
    })
}

pub(crate) fn dispatch(
    config: &Config,
    target: &str,
    args: &[String],
    dry: bool,
) -> crate::cli::Result<i32> {
    let run = || -> Result<i32> {
        ensure!(
            !dry && (target.is_empty() || config.is_self(target)),
            "Acknowledge input locally from a person terminal on its computer"
        );
        ensure!(
            args.len() == 2 && args[1] == "--json",
            "usage: hgs acknowledge-input <session> --json (exact review on stdin)"
        );
        let mut bytes = Vec::new();
        std::io::stdin().take(8193).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 8192,
            "Input acknowledgment exceeds its bound"
        );
        let ack: Acknowledgment = serde_json::from_slice(crate::state::without_bom(&bytes))?;
        ensure!(
            ack.valid() && ack.name == args[0],
            "Invalid input acknowledgment"
        );
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let scope = ui_authority::Scope::new(config, &Location::new(config)?);
            let mut person = Person {
                origin: Origin::Terminal(scope),
                acknowledgment: ack.clone(),
            };
            person.check(&ack).map_err(anyhow::Error::msg)?;
            let reply = outcome(
                &ack,
                crate::state::input_acknowledgment::settle(&ack, &mut person),
            );
            if reply["status"] != "person-acknowledged-unknown" {
                anyhow::bail!(
                    "{}",
                    reply["error"]
                        .as_str()
                        .unwrap_or("Acknowledgment is not confirmed; check Terminal")
                );
            }
            println!("{reply}");
            Ok(0)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        anyhow::bail!("Input acknowledgment is available on Linux and macOS")
    };
    run().map_err(|e| crate::cli::Error::new(1, e.to_string()))
}

/// At most one worker per service, retained through physical settlement and drain.
#[derive(Default)]
pub(super) struct Worker(Option<JoinHandle<()>>);
impl Worker {
    pub fn poll(&mut self) -> bool {
        if self.0.as_ref().is_some_and(JoinHandle::is_finished) {
            let _ = self.0.take().unwrap().join();
        }
        self.0.is_none()
    }
    pub fn start(
        &mut self,
        authority: ui_authority::Validated,
        epoch: String,
        queue: Arc<Queue>,
        seq: u64,
    ) {
        let ack = authority.input_acknowledgment().cloned();
        let refuse = |code| {
            answer(
                &epoch,
                &queue,
                seq,
                json!({"request_id":authority.id(),
                "op":"input_acknowledge","accepted":false,"durable":false,"status":"failed","code":code}),
            );
        };
        if !self.poll() {
            refuse("input_acknowledgment_busy");
            return;
        }
        let Ok(ack) = ack else {
            refuse("frontend_identity_expired");
            return;
        };
        self.0 = Some(std::thread::spawn(move || {
            let mut person = Person {
                origin: Origin::Native(Box::new(authority)),
                acknowledgment: ack.clone(),
            };
            let result = crate::state::input_acknowledgment::settle(&ack, &mut person);
            answer(&epoch, &queue, seq, outcome(&ack, result));
        }));
    }
}
fn answer(epoch: &str, queue: &Queue, seq: u64, value: Value) {
    if let Ok(bytes) = super::protocol::encode(&super::frame(epoch, seq, "response", value)) {
        queue.offer(bytes.into());
    }
    queue.close();
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    op: String,
    request_id: String,
    acknowledgment: Acknowledgment,
}
pub(super) fn query(value: &Value) -> Value {
    let read = || -> Result<Value> {
        let query: Query = serde_json::from_value(value.clone())?;
        ensure!(
            query.op == "input_acknowledge_result"
                && zerus_client_core::input_acknowledgment::canonical(&query.request_id)
                && query.acknowledgment.valid(),
            "Invalid acknowledgment lookup"
        );
        let receipt = crate::state::input_acknowledgment::lookup(&query.acknowledgment)
            .map_err(anyhow::Error::msg)?;
        Ok(
            json!({"op":"input_acknowledge_result","request_id":query.request_id,
            "original_request_id":query.acknowledgment.request_id,"found":receipt.is_some(),"result":receipt}),
        )
    };
    read().unwrap_or_else(|_| json!({"accepted":false,"code":"input_acknowledgment_unavailable"}))
}
