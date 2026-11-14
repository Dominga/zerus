//! Launch-time metadata, never caller authority. Legacy start text is not a pin.
use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) const FIELD: &str = "process_index_pin";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pin {
    pub version: u8,
    pub boot: String,
    pub session: String,
    pub run: String,
    pub agent: String,
    pub child: Process,
    pub child_start: String,
    pub supervisor: Process,
    pub supervisor_start: String,
}
fn string<'a>(record: &'a Value, key: &str) -> &'a str {
    record[key].as_str().unwrap_or("")
}
impl Pin {
    /// The owner supplies identities captured from its original spawn, not an
    /// inventory sample of an already saved record.
    pub(crate) fn capture(
        record: &Value,
        boot: &str,
        child: Process,
        supervisor: Process,
        parent: u32,
    ) -> Result<Self> {
        ensure!(
            parent == supervisor.pid && child.pid != supervisor.pid,
            "Original child/supervisor correlation is unavailable"
        );
        let pin = Self {
            version: 1,
            boot: boot.into(),
            session: string(record, "session_id").into(),
            run: string(record, "run_id").into(),
            agent: string(record, "agent").into(),
            child,
            child_start: string(record, "process_start").into(),
            supervisor,
            supervisor_start: string(&record["supervisor"], "start").into(),
        };
        pin.check(record, boot)?;
        Ok(pin)
    }
    fn check(&self, record: &Value, boot: &str) -> Result<()> {
        ensure!(
            self.version == 1
                && self.boot == boot
                && !boot.is_empty()
                && boot.len() <= 256
                && !boot.chars().any(char::is_control),
            "Launch pin scope changed"
        );
        let id = uuid::Uuid::parse_str(&self.session)?;
        ensure!(
            !id.is_nil()
                && id.to_string() == self.session
                && string(record, "session_id") == self.session,
            "Launch session changed"
        );
        ensure!(
            !self.run.is_empty()
                && self.run.len() <= 512
                && !self.run.chars().any(char::is_control)
                && string(record, "run_id") == self.run,
            "Launch run changed"
        );
        ensure!(
            ["claude", "codex", "kimi"].contains(&self.agent.as_str())
                && string(record, "agent") == self.agent,
            "Launch provider changed"
        );
        self.child.validate()?;
        self.supervisor.validate()?;
        ensure!(
            self.child.pid != self.supervisor.pid
                && record["pid"].as_u64() == Some(u64::from(self.child.pid))
                && record["supervisor"]["pid"].as_u64() == Some(u64::from(self.supervisor.pid)),
            "Launch processes changed"
        );
        ensure!(
            !self.child_start.is_empty()
                && self.child_start.len() <= 256
                && string(record, "process_start") == self.child_start
                && !self.supervisor_start.is_empty()
                && self.supervisor_start.len() <= 256
                && string(&record["supervisor"], "start") == self.supervisor_start,
            "Compatible launch start changed"
        );
        Ok(())
    }
    pub(crate) fn read(record: &Value, boot: &str) -> Result<Option<Self>> {
        record
            .get(FIELD)
            .map(|value| {
                let pin: Self = serde_json::from_value(value.clone())?;
                pin.check(record, boot)?;
                Ok(pin)
            })
            .transpose()
    }
    pub(crate) fn publication(
        &self,
        record: &Value,
        source: &Source,
        locator: &str,
        revision: u64,
        at: u64,
    ) -> Result<Publication> {
        self.check(record, &self.boot)?;
        ensure!(
            source.kind == Kind::Native && source.key == self.session,
            "Launch source changed"
        );
        if record["completion_source"] == "supervisor"
            && record["exited_at"]
                .as_f64()
                .is_some_and(|at| at.is_finite() && at > 0.)
            && (record["exit_code"].as_i64().is_some()
                || record["termination_signal"].as_i64().is_some())
        {
            return Ok(Publication::default());
        }
        let conversation = record["conversation_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(self, &conversation, locator))?)
        );
        let mut result = Publication::default();
        for (process, role) in [
            (&self.child, Role::Agent),
            (&self.supervisor, Role::Supervisor),
        ] {
            result.anchors.push(Anchor {
                process: process.clone(),
                role: role.clone(),
                source: source.clone(),
                revision,
                fingerprint: fingerprint.clone(),
                published_at: at,
            });
            result.edges.push(Edge {
                process: process.clone(),
                role,
                session_id: self.session.clone(),
                run: self.run.clone(),
                conversation: conversation.clone(),
                source: source.clone(),
                revision,
                fingerprint: fingerprint.clone(),
                published_at: at,
            });
        }
        result.validate(source, revision)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (Value, Pin) {
        let record = json!({"session_id":uuid::Uuid::new_v4().to_string(),"run_id":"original-run","agent":"claude","pid":42,"process_start":"original-start","supervisor":{"pid":41,"start":"supervisor-start"},"conversation_id":null});
        let pin = Pin::capture(
            &record,
            "original-boot",
            Process {
                pid: 42,
                birth: Birth::Linux(100),
            },
            Process {
                pid: 41,
                birth: Birth::Linux(90),
            },
            41,
        )
        .unwrap();
        (record, pin)
    }
    #[test]
    fn a_live_pid_and_legacy_start_text_never_manufacture_a_launch_pin() {
        let (record, _) = fixture();
        assert!(Pin::read(&record, "original-boot").unwrap().is_none());
    }
    #[test]
    fn a_supervisor_pin_requires_the_actual_original_child_parent() {
        let (record, pin) = fixture();
        assert!(Pin::capture(&record, &pin.boot, pin.child, pin.supervisor, 900).is_err());
    }
    #[test]
    fn persisted_pins_are_bound_to_the_exact_record_and_boot() {
        let (mut record, pin) = fixture();
        record[FIELD] = serde_json::to_value(&pin).unwrap();
        assert_eq!(Pin::read(&record, &pin.boot).unwrap(), Some(pin.clone()));
        for (key, value) in [
            ("run_id", json!("replacement-run")),
            ("session_id", json!(uuid::Uuid::new_v4().to_string())),
            ("pid", json!(43)),
            ("process_start", json!("replacement-start")),
            ("agent", json!("codex")),
            ("supervisor", json!({"pid":41,"start":"other-birth"})),
        ] {
            let mut changed = record.clone();
            changed[key] = value;
            assert!(
                Pin::read(&changed, &pin.boot).is_err(),
                "changed {key} must remain unqualified"
            );
        }
        assert!(Pin::read(&record, "different-boot").is_err());
    }
    #[test]
    fn conversation_confirmation_updates_edges_without_repinning_the_process() {
        let (mut record, pin) = fixture();
        record[FIELD] = serde_json::to_value(&pin).unwrap();
        let source = Source {
            kind: Kind::Native,
            key: pin.session.clone(),
        };
        let before = pin
            .publication(&record, &source, "root/record.json", 1, 1)
            .unwrap();
        record["conversation_id"] = json!("confirmed-conversation");
        let saved = Pin::read(&record, &pin.boot).unwrap().unwrap();
        let after = saved
            .publication(&record, &source, "root/record.json", 2, 2)
            .unwrap();
        assert_eq!(after.anchors[0].process, before.anchors[0].process);
        assert_ne!(after.anchors[0].fingerprint, before.anchors[0].fingerprint);
        assert!(
            after
                .edges
                .iter()
                .all(|e| e.conversation.as_deref() == Some("confirmed-conversation"))
        );
    }
    #[test]
    fn only_explicit_supervisor_exit_evidence_retires_the_binding() {
        let (mut record, pin) = fixture();
        let source = Source {
            kind: Kind::Native,
            key: pin.session.clone(),
        };
        record["phase"] = json!("interrupted");
        record["paused"] = json!(true);
        assert_eq!(
            pin.publication(&record, &source, "root/record.json", 1, 1)
                .unwrap()
                .anchors
                .len(),
            2
        );
        record["completion_source"] = json!("supervisor");
        record["exited_at"] = json!(123.);
        record["exit_code"] = json!(0);
        assert_eq!(
            pin.publication(&record, &source, "root/record.json", 2, 2)
                .unwrap(),
            Publication::default()
        );
    }
}
