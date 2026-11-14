//! Read-only exact launch binding gate. Hold native writer locks through the
//! caller's metadata transaction. Dsh liveness runs first under its session
//! guard; only binding I/O and the metadata commit take machine-wide locks.
//! This never starts or resumes an agent.
use super::*;

pub(crate) fn with_launch_binding<T>(
    name: &str,
    check: impl FnOnce(&Value) -> std::result::Result<T, anyhow::Error>,
) -> anyhow::Result<T> {
    local_sessions().map_err(anyhow::Error::msg)?;
    if dsh::exists(name) {
        let (_session, record) = dsh::dispatch_lock::acquire(name).map_err(anyhow::Error::msg)?;
        let inspected = dsh::process_rpc(
            &record,
            "inspect",
            json!({
                "sessionId":record["conversation_id"],"includeProcesses":false
            }),
        )
        .map_err(anyhow::Error::msg)?;
        // The bridge inspection alone can return a saved, stopped conversation.
        let listed = dsh::process_rpc(&record, "list", json!({})).map_err(anyhow::Error::msg)?;
        check_dsh_live(&record, &inspected, &listed)?;
        anyhow::ensure!(
            string(&record, "archive_id").is_empty() && record["state"] != "archived",
            "Archived launches cannot be assigned"
        );
        let _writer = lock(None).map_err(anyhow::Error::msg)?;
        return dsh::dispatch_lock::transaction(name, &record, || {
            check(&record).map_err(|error| error.to_string())
        })
        .map_err(anyhow::Error::msg);
    }
    let _writer = lock(None).map_err(anyhow::Error::msg)?;
    let _opencode = opencode::lock_bindings().map_err(anyhow::Error::msg)?;
    let record = if opencode::exists(name) {
        let record = opencode::binding(name).map_err(anyhow::Error::msg)?;
        let inspected = opencode::own_state(name).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            record["paused"] != true && inspected["state"] == "running",
            "Native OpenCode launch is no longer live"
        );
        record
    } else {
        let record = read(name).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            matches(
                &record,
                panes_of(&live().map_err(anyhow::Error::msg)?, &record)
            ),
            "Native launch is no longer live"
        );
        record
    };
    anyhow::ensure!(
        string(&record, "archive_id").is_empty() && record["state"] != "archived",
        "Archived launches cannot be assigned"
    );
    check(&record)
}

/// A saved inspection is not sufficient: the exact native handle must still
/// be listed. Preserve the adapter's explicit false over the legacy fallback.
fn check_dsh_live(record: &Value, inspected: &Value, listed: &Value) -> anyhow::Result<()> {
    let row = listed["items"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["sessionId"] == record["conversation_id"])
        })
        .ok_or_else(|| anyhow::anyhow!("Native launch is no longer present"))?;
    anyhow::ensure!(
        record["paused"] != true
            && inspected["live"]
                .as_bool()
                .unwrap_or(row["agentAvailable"] == true),
        "Native launch is no longer live"
    );
    anyhow::ensure!(!inspected.is_null(), "Native launch cannot be inspected");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Value {
        json!({"conversation_id":"session-one","paused":false})
    }
    fn listed() -> Value {
        json!({"items":[{"sessionId":"session-one","agentAvailable":true}]})
    }

    #[test]
    fn a_saved_inspection_without_the_exact_live_handle_refuses() {
        for list in [
            json!({}),
            json!({"items":[{"sessionId":"session-other","agentAvailable":true}]}),
        ] {
            assert!(check_dsh_live(&record(), &json!({"live":true}), &list).is_err());
        }
    }

    #[test]
    fn a_paused_stopped_or_uninspectable_launch_refuses() {
        for inspection in [Value::Null, json!({"live":false})] {
            assert!(check_dsh_live(&record(), &inspection, &listed()).is_err());
        }
        let mut paused = record();
        paused["paused"] = json!(true);
        assert!(check_dsh_live(&paused, &json!({"live":true}), &listed()).is_err());
    }

    #[test]
    fn only_an_absent_live_flag_uses_the_legacy_availability_fallback() {
        assert!(check_dsh_live(&record(), &json!({}), &listed()).is_ok());
        assert!(
            check_dsh_live(
                &record(),
                &json!({}),
                &json!({"items":[{"sessionId":"session-one","agentAvailable":false}]})
            )
            .is_err()
        );
        assert!(
            check_dsh_live(
                &record(),
                &json!({"live":true}),
                &json!({"items":[{"sessionId":"session-one"}]})
            )
            .is_ok()
        );
    }
}
