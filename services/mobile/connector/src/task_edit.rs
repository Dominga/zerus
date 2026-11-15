//! Only the verified local encrypted receiver installs this context. A wire
//! flag, ordinary exchange or forwarded request cannot manufacture it.
use crate::private_state::Store;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{future::Future, path::PathBuf, rc::Rc, time::Duration};
use zerus_mobile_crypto::EncryptedEnvelope;
use zerus_mobile_endpoint::native_journal::{retained_after_restart, Outcome};

pub(crate) const NO_PROOF:&str="The original mobile edit has no matching durable native result; its outcome is unknown. Keep the draft and sync this task for review";

#[derive(Clone)]
pub(crate) struct Context {
    pub(crate) envelope: Rc<EncryptedEnvelope>,
    private_dir: PathBuf,
    machine: String,
}
tokio::task_local! { static CONTEXT:Context; }
pub(crate) async fn guarded<T>(
    private_dir: PathBuf,
    machine: String,
    envelope: EncryptedEnvelope,
    future: impl Future<Output = T>,
) -> T {
    CONTEXT
        .scope(
            Context {
                envelope: Rc::new(envelope),
                private_dir,
                machine,
            },
            future,
        )
        .await
}
pub(crate) fn context() -> Option<Context> {
    CONTEXT.try_with(Clone::clone).ok()
}

/// Recognize the reserved version family, including unsupported versions. An
/// invalid/newer frame must not fall through into ordinary document import.
pub(crate) fn claims(payload: &Value) -> bool {
    payload
        .get("updates")
        .and_then(Value::as_array)
        .is_some_and(|updates| {
            updates.iter().take(257).any(|update| {
                let Some(encoded) = update.get("bytes").and_then(Value::as_str) else {
                    return false;
                };
                let prefix = if encoded.len() >= 16 {
                    encoded.get(..16)
                } else {
                    encoded.get(..12)
                };
                prefix
                    .and_then(|prefix| STANDARD.decode(prefix).ok())
                    .is_some_and(|bytes| bytes.starts_with(b"ZERUSME"))
            })
        })
}

impl Context {
    pub(crate) async fn completion(&self) -> Result<Value, &'static str> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let store = loop {
            match Store::try_lock(&self.private_dir, false) {
                Ok(Some(store)) => break store,
                Ok(None) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                _ => return Err(NO_PROOF),
            }
        };
        let digest = self.envelope.digest().map_err(|_| NO_PROOF)?;
        recovered(&store, &self.envelope.header, &digest, &self.machine)
    }
}

pub(crate) fn recovered(
    store: &Store,
    header: &zerus_mobile_crypto::Header,
    digest: &str,
    machine: &str,
) -> Result<Value, &'static str> {
    let outcome = retained_after_restart(store, header, digest, machine)
        .map_err(|_| NO_PROOF)?
        .ok_or(NO_PROOF)?;
    if outcome == Outcome::Unknown {
        return Err(NO_PROOF);
    }
    let mut answer = serde_json::to_value(outcome).map_err(|_| NO_PROOF)?;
    answer["schema"] = json!(1);
    answer["kind"] = json!("mobile_task_edit");
    answer["request_id"] = json!(header.request_id);
    answer["envelope_digest"] = json!(digest);
    Ok(answer)
}

pub(crate) fn advertise(snapshot: &mut Value, eligible: bool) {
    let Some(features) = snapshot
        .pointer_mut("/mobile_capabilities/features")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    features.retain(|feature| feature != crate::capabilities::TASK_EDIT_FEATURE);
    if eligible {
        features.push(json!(crate::capabilities::TASK_EDIT_FEATURE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_qualified_local_encrypted_snapshot_advertises_admitted_edits() {
        let mut local = json!({"mobile_capabilities":{"features":["task_media_upload_v1"]}});
        advertise(&mut local, true);
        assert!(local["mobile_capabilities"]["features"]
            .as_array()
            .unwrap()
            .contains(&json!(crate::capabilities::TASK_EDIT_FEATURE)));
        let mut forwarded = local.clone();
        advertise(&mut forwarded, false);
        assert_eq!(
            forwarded,
            json!({"mobile_capabilities":{"features":["task_media_upload_v1"]}})
        );
        for mut malformed in [
            json!({}),
            json!({"mobile_capabilities":{"features":"invalid"}}),
        ] {
            let original = malformed.clone();
            advertise(&mut malformed, true);
            assert_eq!(malformed, original);
        }
    }
}
