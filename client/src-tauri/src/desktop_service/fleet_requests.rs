//! The background service's own connection supervisors: Reconnect now ends
//! a computer's restart wait there, and saved reconnect timings are re-read.
//! Connection observation only: no activation, ownership, permission or
//! restart of the service, a source or an agent. An unknown outcome is
//! reported, never sent again.
use super::*;

/// The most computers one request names, as the service accepts.
const COMPUTERS: usize = 64;

impl Service {
    /// Ends `computer`'s restart wait in the service's existing supervisor;
    /// `""` is this computer. `incarnation` binds it to the source the
    /// person saw. Returns the service's code (`requested`, `pending`,
    /// `connected`, `not_configured`, `not_admitted`, `source_changed` or
    /// `stopping`).
    pub async fn fleet_reconnect(
        &self,
        computer: &str,
        incarnation: Option<u64>,
    ) -> Result<String, &'static str> {
        let reply = self
            .fleet_request(json!({"op":"fleet_reconnect",
                "computers":[{"computer":computer,"incarnation":incarnation}]}))
            .await?;
        let result = reply["results"]
            .as_array()
            .filter(|results| results.len() == 1)
            .map(|results| &results[0])
            .filter(|result| result["computer"] == computer)
            .ok_or("invalid_service_response")?;
        result["code"]
            .as_str()
            .filter(|code| code.len() <= 64)
            .map(str::to_owned)
            .ok_or("invalid_service_response")
    }

    /// Makes the service re-read the saved reconnect timings and returns the
    /// ones it now uses (`delays_seconds`, `source`, `diagnostic`).
    pub async fn reload_retry_delays(&self) -> Result<Value, &'static str> {
        let reply = self
            .fleet_request(json!({"op":"fleet_retry_delays_reload"}))
            .await?;
        Ok(reply["retry_delays"].clone())
    }

    async fn fleet_request(&self, mut value: Value) -> Result<Value, &'static str> {
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err("service_unavailable");
        }
        let id = uuid::Uuid::new_v4().to_string();
        value["request_id"] = json!(id);
        let reply = request(&self.shared, value, Arc::new(AtomicBool::new(false))).await?;
        if reply["request_id"] != id {
            return Err("invalid_service_response");
        }
        if reply["accepted"] != true {
            return Err(match reply["code"].as_str() {
                Some("fleet_unavailable") => "fleet_unavailable",
                Some("invalid_request") => "invalid_request",
                // An older service has no such request.
                _ => "fleet_request_unsupported",
            });
        }
        Ok(reply)
    }

    /// The existing Refresh paths also end the service supervisors' waits
    /// (see [`targets`]). Sent once in the background; its outcome is not
    /// awaited or repeated.
    pub(super) fn nudge_fleet(&self, targets: Vec<Value>) {
        if targets.is_empty() {
            return;
        }
        let shared = self.shared.clone();
        let value = json!({"op":"fleet_reconnect","request_id":uuid::Uuid::new_v4().to_string(),
            "computers":targets});
        tauri::async_runtime::spawn(async move {
            let _ = request(&shared, value, Arc::new(AtomicBool::new(false))).await;
        });
    }
}

/// The computers a Refresh names: `computer`, or with `""` every configured
/// one the service reports as not connected, bound to its source.
pub(super) fn targets(snapshot: &Snapshot, computer: &str) -> Vec<Value> {
    if !computer.is_empty() {
        return vec![json!({"computer":computer})];
    }
    snapshot
        .fleet_computers
        .iter()
        .flatten()
        .filter(|row| !matches!(row.status.as_str(), "live" | "polled"))
        .take(COMPUTERS)
        .map(|row| json!({"computer":row.id,"incarnation":row.incarnation}))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_client_core::desktop::service::FleetComputer;

    fn row(id: &str, status: &str, incarnation: Option<u64>) -> FleetComputer {
        FleetComputer {
            id: id.into(),
            status: status.into(),
            attempt: 1,
            incarnation,
            retry: None,
        }
    }

    #[test]
    fn a_refresh_names_only_the_computers_that_are_not_connected() {
        let snapshot = Snapshot {
            fleet_computers: Some(vec![
                row("", "live", Some(1)),
                row("studio", "reconnecting", Some(4)),
                row("build", "polled", Some(5)),
                row("late", "probing", None),
            ]),
            ..Snapshot::default()
        };
        assert_eq!(
            targets(&snapshot, ""),
            vec![
                json!({"computer":"studio","incarnation":4}),
                json!({"computer":"late","incarnation":null}),
            ]
        );
        assert_eq!(
            targets(&snapshot, "studio"),
            vec![json!({"computer":"studio"})]
        );
        assert!(targets(&Snapshot::default(), "").is_empty());
    }
}
