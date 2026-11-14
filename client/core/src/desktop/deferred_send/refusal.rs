//! Retained evidence from the CLI that this exact original refused before input.
//! It is a diagnostic, never a submission, no-spawn proof or automatic desktop retry permit.
use super::*;

pub const STATUS: &str = "refused_before_input";
pub const CODE: &str = "deferred_send_refused_before_input";
pub const BYTES: usize = 8192;
pub const MESSAGE: &str = crate::source!("Message was not sent. hgs refused before typing: %1");

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub schema: u32,
    pub request_id: String,
    pub session: String,
    pub run_id: String,
    pub conversation_id: String,
    pub input_sha256: String,
    pub reason: String,
}

#[derive(Deserialize)]
struct CliRefusal {
    refusal_schema: u32,
    request_id: String,
    name: String,
    run_id: String,
    conversation_id: String,
    first_message: bool,
    digest: String,
    status: String,
    transport: String,
    reason: String,
}

impl Refusal {
    /// Bound and sanitize only the diagnostic; identities are never repaired.
    pub fn new(
        request_id: &str,
        session: &str,
        run_id: &str,
        conversation_id: &str,
        input_sha256: &str,
        reason: &str,
    ) -> Option<Self> {
        let value = Self {
            schema: 1,
            request_id: request_id.into(),
            session: session.into(),
            run_id: run_id.into(),
            conversation_id: conversation_id.into(),
            input_sha256: input_sha256.into(),
            reason: CliExit::new(1, reason).stderr,
        };
        value.valid().then_some(value)
    }
    pub fn valid(&self) -> bool {
        self.schema == 1
            && uuid(&self.request_id)
            && local_session(&self.session)
            && identity(&self.run_id)
            && !self.run_id.is_empty()
            && identity(&self.conversation_id)
            && super::super::digest(&self.input_sha256)
            && !self.reason.trim().is_empty()
            && self.reason.len() <= STDERR_BYTES
            && !self.reason.chars().any(char::is_control)
    }
    pub fn matches(&self, plan: &Plan) -> bool {
        self.valid()
            && self.request_id == plan.request_id
            && self.session == plan.session
            && self.run_id == plan.run_id
            && self.conversation_id == plan.conversation_id
            && self.input_sha256 == plan.input_sha256
    }
    pub(super) fn agrees(&self, binding: &Binding) -> bool {
        self.valid()
            && self.request_id == binding.request_id
            && self.session == binding.session
            && self.run_id == binding.run_id
            && self.conversation_id == binding.conversation_id
            && self.input_sha256 == binding.input_sha256
    }
    /// The public send writes this only while it still owns the input lock and
    /// its live NotOffered witness. A duplicate UUID is thereafter spent.
    pub fn cli_record(&self) -> Option<serde_json::Value> {
        self.valid().then(|| {
            serde_json::json!({
                "refusal_schema":self.schema,"request_id":self.request_id,"name":self.session,
                "run_id":self.run_id,"conversation_id":self.conversation_id,
                "first_message":self.conversation_id.is_empty(),"digest":self.input_sha256,
                "status":STATUS,"transport":"terminal","reason":self.reason
            })
        })
    }
    /// Decode the original bytes, retaining duplicate-field rejection. No exit,
    /// stdout, absence or partial identity can manufacture this diagnostic.
    pub fn from_receipt(plan: &Plan, bytes: &[u8]) -> Result<Self, &'static str> {
        let value = Self::from_cli_record(bytes)?;
        value.matches(plan).then_some(value).ok_or(RECEIPT_MISMATCH)
    }

    /// Decode bounded negative evidence without granting any delivery or retry
    /// authority. A public CLI retry must separately check every original binding.
    pub fn from_cli_record(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > BYTES {
            return Err(RECEIPT_INVALID);
        }
        let raw: CliRefusal = serde_json::from_slice(bytes).map_err(|_| RECEIPT_INVALID)?;
        if raw.status != STATUS
            || raw.transport != "terminal"
            || raw.first_message != raw.conversation_id.is_empty()
        {
            return Err(RECEIPT_MISMATCH);
        }
        let value = Self {
            schema: raw.refusal_schema,
            request_id: raw.request_id,
            session: raw.name,
            run_id: raw.run_id,
            conversation_id: raw.conversation_id,
            input_sha256: raw.digest,
            reason: raw.reason,
        };
        if !value.valid() {
            return Err(RECEIPT_INVALID);
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn original() -> (super::super::Verified, Refusal, Value) {
        let v = super::super::tests::intent().verify().unwrap();
        let p = &v.plan;
        let refusal = Refusal::new(
            &p.request_id,
            &p.session,
            &p.run_id,
            &p.conversation_id,
            &p.input_sha256,
            "a verified composer draft",
        )
        .unwrap();
        let record = refusal.cli_record().unwrap();
        (v, refusal, record)
    }
    #[test]
    fn retained_refusal_requires_every_original_binding_and_a_bounded_reason() {
        let (v, refusal, record) = original();
        assert_eq!(
            Refusal::from_receipt(&v.plan, record.to_string().as_bytes()),
            Ok(refusal)
        );
        for (key, value) in [
            ("request_id", json!("00000000-0000-4000-8000-000000000001")),
            ("name", json!("claude/private-other")),
            ("run_id", json!("other-run")),
            ("conversation_id", json!("other-conversation")),
            ("digest", json!("f".repeat(64))),
            ("transport", json!("pipe")),
            ("first_message", json!(true)),
            ("status", json!("submitted")),
            ("refusal_schema", json!(2)),
            ("reason", json!("")),
            ("reason", json!("\u{1b}[31munsafe")),
            ("reason", json!("x".repeat(STDERR_BYTES + 1))),
        ] {
            let mut bad = record.clone();
            bad[key] = value;
            assert!(
                Refusal::from_receipt(&v.plan, bad.to_string().as_bytes()).is_err(),
                "{key}"
            );
        }
        for key in [
            "request_id",
            "name",
            "run_id",
            "conversation_id",
            "digest",
            "transport",
            "status",
            "refusal_schema",
            "reason",
            "first_message",
        ] {
            let mut missing = record.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(
                Refusal::from_receipt(&v.plan, missing.to_string().as_bytes()).is_err(),
                "missing {key}"
            );
            let body = record.to_string();
            let duplicate = format!("{{\"{key}\":{},{}", record[key], &body[1..]);
            assert!(
                Refusal::from_receipt(&v.plan, duplicate.as_bytes()).is_err(),
                "duplicate {key}"
            );
        }
        let mut large = record.clone();
        large["unused"] = json!("x".repeat(BYTES));
        assert!(Refusal::from_receipt(&v.plan, large.to_string().as_bytes()).is_err());
    }
    #[test]
    fn a_refusal_never_validates_as_submission_or_no_spawn_and_old_receipts_stay_compatible() {
        let (v, refusal, record) = original();
        assert!(Outcome::from_receipt(&v.plan, record.to_string().as_bytes()).is_err());
        let mut disguised = record;
        disguised["status"] = json!(SUBMITTED);
        assert!(
            Outcome::from_receipt(&v.plan, disguised.to_string().as_bytes()).is_err(),
            "negative schema cannot become submission"
        );
        let mut receipt = Receipt {
            binding: v.binding.clone(),
            admission_epoch: v.intent.scope.service_epoch.clone(),
            revision: 3,
            phase: Phase::Unknown,
            drained: true,
            code: CODE.into(),
            outcome: None,
            cli_exit: None,
            refusal: Some(refusal),
        };
        assert!(receipt.valid());
        assert_eq!(receipt.result(&v.plan), None);
        assert!(!receipt.not_started(&v.plan));
        receipt.phase = Phase::Recorded;
        receipt.outcome = Some(Outcome {
            request_id: v.plan.request_id.clone(),
            status: SUBMITTED.into(),
            input_sha256: v.plan.input_sha256.clone(),
        });
        assert!(
            !receipt.valid(),
            "negative evidence cannot validate as delivery"
        );
        assert_ne!(receipt.result(&v.plan), Some(Ok(())));
        receipt.phase = Phase::Failed;
        receipt.outcome = None;
        assert!(!receipt.valid());
        receipt.refusal = None;
        receipt.code = NOT_STARTED.into();
        assert!(receipt.valid());
        let old = serde_json::to_string(&receipt).unwrap();
        assert!(!old.contains("\"refusal\""));
        assert_eq!(
            serde_json::to_string(&serde_json::from_str::<Receipt>(&old).unwrap()).unwrap(),
            old
        );
    }
    #[test]
    fn writer_sanitizes_only_the_reason_and_keeps_utf8_and_identity_boundaries() {
        let (v, _, _) = original();
        let p = &v.plan;
        let value = Refusal::new(
            &p.request_id,
            &p.session,
            &p.run_id,
            &p.conversation_id,
            &p.input_sha256,
            &("\u{1b}".to_owned() + &"ж".repeat(1024) + "\nprivate second line"),
        )
        .unwrap();
        assert_eq!(value.reason.len(), STDERR_BYTES);
        assert!(!value.reason.chars().any(char::is_control));
        assert!(!value.reason.contains("private"));
        assert!(Refusal::new(
            &p.request_id,
            &p.session,
            "",
            &p.conversation_id,
            &p.input_sha256,
            "reason"
        )
        .is_none());
    }
}
