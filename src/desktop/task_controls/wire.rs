//! Closed local Hello payload. Parsing grants neither Person/Executor identity
//! nor native ownership. Only the service's held peer proof can supply those.
use crate::tasks::controls::wire::Command;
use serde::{Deserialize, Serialize};
use zerus_desktop_ledger::task_controls::REQUEST_BYTES;

pub(in crate::desktop) const OP: &str = "task_control";
pub(super) const VERSION: u32 = 1;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    pub schema: u32,
    pub request_id: String,
    pub command: Command,
}
impl Envelope {
    /// Keep original JSON: a generic Hello Value would erase duplicate fields
    /// and whitespace before the closed, bounded envelope reader.
    pub fn hello(bytes: &[u8], configuration: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Hello<'a> {
            abi: &'a str,
            configuration: &'a str,
            op: &'a str,
            #[serde(borrow)]
            request: &'a serde_json::value::RawValue,
        }
        let hello: Hello<'_> = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if hello.abi != crate::desktop::protocol::ABI
            || hello.configuration != configuration
            || hello.op != OP
        {
            return Err("Invalid original task-control service scope".into());
        }
        Self::read(hello.request.get().as_bytes())
    }
    pub fn new(command: Command) -> Result<Self, String> {
        let id = match &command {
            Command::Plan(p) => p.request_id.clone(),
            Command::Execute(p) => p.request.id.clone(),
            // An acknowledgement keeps its immutable attempt proof separate
            // from its local admission; a status request creates no effect.
            Command::Begin(_) | Command::Status(_) => uuid::Uuid::new_v4().to_string(),
        };
        let value = Self {
            schema: VERSION,
            request_id: id,
            command,
        };
        value.validate()?;
        value.encoded()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), String> {
        if self.schema != VERSION
            || !uuid::Uuid::parse_str(&self.request_id)
                .is_ok_and(|id| id.to_string() == self.request_id && !id.is_nil())
        {
            return Err("Unsupported task-control service version or request identity".into());
        }
        self.command.validate()?;
        let original = match &self.command {
            Command::Plan(p) => Some(&p.request_id),
            Command::Execute(p) => Some(&p.request.id),
            _ => None,
        };
        if original.is_some_and(|id| *id != self.request_id) {
            return Err("The original task-control request changed".into());
        }
        Ok(())
    }
    pub fn encoded(&self) -> Result<Vec<u8>, String> {
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if bytes.len() > REQUEST_BYTES {
            return Err("Task-control service request exceeds 16 KiB".into());
        }
        Ok(bytes)
    }
    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > REQUEST_BYTES {
            return Err("Task-control service request exceeds 16 KiB".into());
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    const ID: &str = "00000000-0000-4000-8000-000000000001";
    fn read(value: &Value) -> Result<Envelope, String> {
        Envelope::read(&serde_json::to_vec(value).unwrap())
    }
    fn status() -> Value {
        json!({"schema":1,"request_id":ID,"command":{"operation":"status","input":{"schema":1,"target":{"scope":"request","request_id":ID}}}})
    }
    #[test]
    fn the_existing_local_abi_carries_only_the_closed_bounded_envelope() {
        let original = status();
        let parsed = read(&original).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&parsed.encoded().unwrap()).unwrap(),
            original
        );
        let mut value = original.clone();
        value["actor"] = "person".into();
        assert!(read(&value).is_err());
        value = original.clone();
        value["command"]["surprise"] = true.into();
        assert!(read(&value).is_err());
        value = original.clone();
        value["command"]["input"]["actor"] = "person".into();
        assert!(read(&value).is_err());
        assert!(Envelope::read(&vec![b' '; REQUEST_BYTES + 1]).is_err());
        let mut future = original;
        future["schema"] = 2.into();
        assert!(read(&future).is_err());
    }
    #[test]
    fn begin_keeps_the_original_attempt_instead_of_an_inferred_current_origin() {
        let input = Command::Begin(crate::tasks::controls::wire::Begin {
            reference: "orchard/T-234567".into(),
            attempt: ID.into(),
        });
        let value = Envelope::new(input).unwrap();
        assert_ne!(value.request_id, ID);
        let Command::Begin(begin) = Envelope::read(&value.encoded().unwrap()).unwrap().command
        else {
            panic!("expected begin")
        };
        assert_eq!(begin.attempt, ID);
        let mut substituted = serde_json::to_value(value).unwrap();
        substituted["command"]["input"]["attempt"] = "latest".into();
        assert!(read(&substituted).is_err());
    }
    #[test]
    fn control_plan_identity_is_the_execute_identity_not_an_outer_replacement() {
        let original = json!({"schema":1,"request_id":ID,"command":{"operation":"plan","input":{"schema":1,"request_id":ID,"action":"pause","target":{"scope":"attempt","project":"orchard","task":ID,"attempt":ID},"observed_pauses":[],"observed_stops":[]}}});
        assert!(read(&original).is_ok());
        let mut changed = original;
        changed["request_id"] = uuid::Uuid::new_v4().to_string().into();
        assert!(read(&changed).is_err());
    }
}

#[cfg(test)]
mod hello_tests {
    use super::*;
    #[test]
    fn original_hello_preserves_duplicate_field_and_raw_size_refusals() {
        let id = "00000000-0000-4000-8000-000000000001";
        let body = format!(
            r#"{{"schema":1,"request_id":"{id}","command":{{"operation":"status","input":{{"schema":1,"target":{{"scope":"request","request_id":"{id}"}}}}}}}}"#
        );
        let hello = |body: &str| {
            format!(
                r#"{{"abi":"desktop-v1","configuration":"{}","op":"task_control","request":{body}}}"#,
                "a".repeat(64)
            )
        };
        assert!(Envelope::hello(hello(&body).as_bytes(), &"a".repeat(64)).is_ok());
        let duplicate = body.replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1);
        assert!(Envelope::hello(hello(&duplicate).as_bytes(), &"a".repeat(64)).is_err());
        let duplicate = body.replace(
            "\"scope\":\"request\"",
            "\"scope\":\"executor\",\"scope\":\"request\"",
        );
        assert!(Envelope::hello(hello(&duplicate).as_bytes(), &"a".repeat(64)).is_err());
        let padded = body.replacen('{', &("{".to_owned() + &" ".repeat(REQUEST_BYTES)), 1);
        assert!(Envelope::hello(hello(&padded).as_bytes(), &"a".repeat(64)).is_err());
        assert!(Envelope::hello(hello(&body).as_bytes(), &"b".repeat(64)).is_err());
    }
}
