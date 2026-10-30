//! Read-only GUI health; no frontend writer, producer admission or drain proof.
use super::store::Location;
use serde_json::{json, Value};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
#[cfg(unix)]
use zerus_client_core::desktop;
use zerus_client_core::desktop::producer::Observation;

pub fn inspect(configuration: &str) -> Value {
    #[cfg(unix)]
    let observation = desktop::probe_producer_at(
        &desktop::temporary_dir_from(std::env::var_os("TMPDIR")).join("hgs-tray"),
        &uuid::Uuid::new_v4().to_string(),
        configuration,
    );
    #[cfg(not(unix))]
    let observation = {
        let _ = configuration;
        Observation::Unknown
    };
    match observation {
        Observation::LiveNext { reply } => json!({"state":"live_next",
            "instance_epoch":reply.hello.instance_epoch,"read_delivery":reply.read_delivery,"authority":false}),
        Observation::Absent => json!({"state":"absent","read_delivery":null,"authority":false}),
        Observation::Unknown => unknown(),
        Observation::LiveLegacyOrUnknown => {
            json!({"state":"live_legacy_or_unknown","read_delivery":null,"authority":false})
        }
    }
}
fn unknown() -> Value {
    json!({"state":"unknown","read_delivery":null,"authority":false})
}

/// At most one outstanding off-actor probe, one cached result. Even blocked
/// filesystem I/O cannot stack workers. Ten-second spacing; stale is unknown.
pub struct Cache {
    pending: Option<mpsc::Receiver<Value>>,
    latest: Option<(Instant, Value)>,
    next: Instant,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            pending: None,
            latest: None,
            next: Instant::now(),
        }
    }
}
impl Cache {
    pub fn tick(&mut self, location: &Location) {
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(value) => {
                    self.latest = Some((Instant::now(), value));
                    self.pending = None;
                    self.next = Instant::now() + Duration::from_secs(10);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.next = Instant::now() + Duration::from_secs(10);
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        if self.pending.is_some() || Instant::now() < self.next {
            return;
        }
        let configuration = location.configuration.clone();
        let (send, receive) = mpsc::sync_channel(1);
        self.next = Instant::now() + Duration::from_secs(10);
        if thread::Builder::new()
            .name("desktop-frontend-health".into())
            .spawn(move || {
                let _ = send.send(inspect(&configuration));
            })
            .is_ok()
        {
            self.pending = Some(receive);
        }
    }
    pub fn status(&self) -> Value {
        self.latest
            .as_ref()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(15))
            .map(|(_, value)| value.clone())
            .unwrap_or_else(unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_stale_health_never_becomes_a_current_count() {
        let mut cache = Cache::default();
        assert_eq!(cache.status()["state"], "unknown");
        let value = json!({"state":"live_next","read_delivery":{"pinned":4},"authority":false});
        cache.latest = Some((Instant::now(), value.clone()));
        assert_eq!(cache.status(), value);
        cache.latest = Some((Instant::now() - Duration::from_secs(16), value));
        assert_eq!(cache.status()["state"], "unknown");
        assert_eq!(cache.status()["read_delivery"], Value::Null);
    }
}
