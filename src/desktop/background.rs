//! Service-owned upstream lifetime. The actor applies every original event.
use super::{
    attention, delivery, details,
    fanout::Queue,
    feed, fleet,
    notifications::activation,
    presence_worker,
    store::{maintenance, Location, Store},
};
use crate::config::Config;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const POLL_EVENTS: usize = 16;

pub(super) struct Runtime {
    fleet: fleet::Runtime,
    producer: Arc<feed::Producer>,
    presence: Option<presence_worker::Runtime>,
    delivery: delivery::Runtime,
    details: details::Adapter,
    maintenance: maintenance::Runtime,
    attention: attention::Runtime,
    activations: activation::Router,
}

impl Runtime {
    pub fn start(config: &Config, epoch: &str, broker: &mut feed::Broker) -> Result<Self> {
        let location = Location::new(config)?;
        let delivery = delivery::Runtime::start(config, epoch)?;
        let maintenance = maintenance::Runtime::start(config, epoch)?;
        let producer = broker.claim_internal()?;
        match fleet::Runtime::start(config, epoch) {
            Ok(fleet) => Ok(Self {
                fleet,
                producer,
                presence: None,
                delivery,
                details: details::Adapter::new(epoch),
                maintenance,
                attention: attention::Runtime::new(epoch),
                activations: activation::Router::new(&location.configuration, epoch),
            }),
            Err(error) => {
                producer.close();
                Err(error)
            }
        }
    }

    pub fn start_presence(&mut self, location: &Location) -> Result<()> {
        self.presence = Some(presence_worker::Runtime::start(location)?);
        Ok(())
    }

    /// Each original permit is held through the transaction and observation
    /// point. At most sixteen packets are processed per actor turn.
    pub fn poll(
        &mut self,
        store: &mut Store,
        broker: &mut feed::Broker,
        seq: &mut u64,
    ) -> (Vec<Value>, bool) {
        let fleet = &self.fleet;
        self.details.tick(
            &mut |computer, generation, controls| fleet.controls(computer, generation, controls),
            seq,
        );
        let mut changes = Vec::new();
        let mut consumed = 0;
        for _ in 0..POLL_EVENTS {
            let Some(event) = self.fleet.poll() else {
                break;
            };
            consumed += 1;
            match broker.apply(store, &self.producer, &event.packet) {
                Ok(change) => {
                    self.delivery.observe(store, &event);
                    self.maintenance.observe(store, &event);
                    if self.attention.enabled() {
                        self.attention.observe(store, &event);
                    }
                    let fleet = &self.fleet;
                    self.details.observe(
                        &event,
                        &mut |computer, generation, controls| {
                            fleet.controls(computer, generation, controls)
                        },
                        seq,
                    );
                    if let Some(change) = change {
                        changes.push(change);
                    }
                }
                Err(_) => {
                    self.maintenance.reject(&event);
                    self.attention.reject_event(&event);
                    self.details.reject(&event, seq);
                    broker.invalidate(&self.producer, &event.packet);
                    self.fleet.reject(&event);
                }
            }
        }
        self.delivery.poll(store);
        self.maintenance.poll(store);
        self.activations.poll(store, None, seq);
        // Disabled discovery must not accumulate permanent edge/baseline rows.
        // Enabling resets the baseline before later observations may post.
        if self.attention.enabled() || self.attention.retiring() {
            self.attention.poll(store, None);
        }
        (
            changes,
            consumed == POLL_EVENTS || self.attention.work_pending(),
        )
    }

    pub fn delivery_request(&mut self, store: &mut Store, value: &Value) -> Value {
        self.delivery.request(store, value)
    }

    pub fn activation_wait(
        &mut self,
        authority: super::ui_authority::Validated,
        queue: Arc<Queue>,
    ) -> Result<()> {
        self.activations.wait(authority, queue)
    }

    pub fn frontend_request(
        &mut self,
        store: &mut Store,
        authority: super::ui_authority::Validated,
    ) -> Value {
        if authority.operation() == "automatic_approvals" {
            return self.delivery.frontend_request(store, authority);
        }
        self.maintenance.frontend_request(store, authority)
    }

    pub fn details_open(&mut self, queue: &Arc<Queue>) -> Option<Value> {
        self.details.open(queue)
    }
    pub fn details_request(&mut self, store: &Store, request: &Value, seq: &mut u64) -> Value {
        let fleet = &self.fleet;
        self.details.request(
            store,
            request,
            &mut |computer, generation, controls| fleet.controls(computer, generation, controls),
            seq,
        )
    }

    pub fn decorate(&self, value: &mut Value) {
        value["detail_subscriptions"] = self.details.status();
        value["attention_discovery"] = self.attention.status();
        value["activation_transport"] = self.activations.status();
        value["lifecycle"]["background"]["attention"] = json!({
            "ready":false,"drained":self.attention.drained(),"code":if self.attention.drained() {"drained"} else {"discovery_only"},
            "status":value["attention_discovery"]});
        value["maintenance"] = self.maintenance.status();
        value["components"]["maintenance"] = if value["maintenance"]["ready"] == true {
            "running"
        } else {
            "unavailable"
        }
        .into();
        value["lifecycle"]["background"]["maintenance"] = value["maintenance"].clone();
        let status = self.fleet.status();
        let ready = status["transport_ready"] == true;
        let code = match status["state"].as_str() {
            Some("drained") => "drained",
            Some("stopping") => "draining",
            _ if ready => "running",
            _ => "starting_or_degraded",
        };
        let presence = self.presence.as_ref().map(presence_worker::Runtime::status);
        let delivery = self.delivery.status();
        self.delivery.decorate(value);
        value["components"]["reads"] = value["lifecycle"]["background"]["reads"]["code"].clone();
        value["components"]["approvals"] =
            value["lifecycle"]["background"]["approvals"]["code"].clone();
        value["lifecycle"]["background"]["delivery"] = delivery.clone();
        value["delivery_runtime"] = delivery.clone();
        value["ownership"] = if code == "drained"
            && delivery["drained"] == true
            && self.maintenance.drained()
            && self.attention.drained()
            && presence.as_ref().is_none_or(|p| p["drained"] == true)
        {
            "revoked"
        } else if presence.is_some() {
            "background_service"
        } else {
            "background_readonly"
        }
        .into();
        value["upstreams"] = status["computers"].clone();
        value["background_runtime"] = status.clone();
        value["components"]["fleet"] = code.into();
        value["lifecycle"]["background"]["fleet"] = json!({"ready":ready,
            "code":code,"status":status});
        if let Some(presence) = presence {
            value["writers_enabled"] = presence["writer_active"].clone();
            value["components"]["presence"] = presence["code"].clone();
            value["lifecycle"]["background"]["presence"] = presence;
        }
    }

    pub fn drain(&mut self, store: &mut Store, timeout: Duration) -> Result<()> {
        self.activations.stop();
        self.details.stop();
        self.producer.close();
        self.delivery.request_stop();
        self.maintenance.request_stop();
        self.attention.request_stop();
        if let Some(presence) = &self.presence {
            presence.request_stop();
        }
        let end = Instant::now() + timeout;
        let mut failure = None;
        loop {
            let fleet_done = self.fleet.stop();
            self.delivery.poll(store);
            self.maintenance.poll(store);
            let delivery_done = self.delivery.drained();
            let maintenance_done = self.maintenance.drained();
            let attention_done = self.attention.drained();
            let presence_done = match self.presence.as_mut().map(presence_worker::Runtime::stop) {
                None | Some(Ok(true)) => true,
                Some(Ok(false)) => false,
                Some(Err(error)) => {
                    failure = Some(error);
                    true // join failed, but still finish draining the fleet
                }
            };
            if fleet_done && presence_done && delivery_done && maintenance_done && attention_done {
                return failure.map_or(Ok(()), Err);
            }
            ensure!(
                Instant::now() < end,
                "desktop background shutdown is incomplete; owned workers have not drained"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
