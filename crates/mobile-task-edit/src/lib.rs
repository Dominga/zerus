//! The authenticated local mobile lane. Its workspace and replica paths are
//! native selections; no request DTO may choose a trust root or a store path.
pub use zerus_replica::{Error, Result};
fn fail(error: impl std::fmt::Display) -> Error {
    Error::new(error.to_string())
}
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    path::PathBuf,
    time::{Duration, Instant},
};
use zerus_mobile_crypto::DevicePublic;
pub use zerus_mobile_crypto::EncryptedEnvelope;
use zerus_mobile_endpoint::{
    authorize,
    native_journal::{self, Admission, Outcome, TaskBinding},
    private_state::Store,
    Profile, VerifiedRequest,
};
use zerus_replica::{
    mobile_edit::{
        wire::{Command, Frame},
        Baseline, Receipt,
    },
    phone::WireUpdate,
    Inventory, ProjectStore,
};

const WAIT: Duration = Duration::from_secs(2);

pub struct Endpoint {
    replica_root: PathBuf,
    private_dir: PathBuf,
    machine: String,
    configuration: Value,
}

impl Endpoint {
    /// Only native code selects these paths. Neither the encrypted request nor
    /// a deserialized phone claim supplies a configuration/store location.
    pub fn configured(
        replica_root: PathBuf,
        configuration_path: PathBuf,
        machine: String,
    ) -> Result<Self> {
        let (configuration, _) =
            zerus_mobile_endpoint::configuration::read(&configuration_path).map_err(fail)?;
        let private_dir = configuration_path
            .parent()
            .filter(|path| path.is_absolute())
            .ok_or_else(|| fail("the native mobile configuration is unavailable"))?
            .join("workspace-private");
        Ok(Self {
            replica_root,
            private_dir,
            machine,
            configuration,
        })
    }
    pub fn execute(&self, envelope: &EncryptedEnvelope, now: u64) -> Result<Value> {
        execute(self, envelope, now)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    schema: u32,
    project: String,
    inventory: Inventory,
    updates: Vec<WireUpdate>,
}

fn execute(scope: &Endpoint, envelope: &EncryptedEnvelope, now: u64) -> Result<Value> {
    let deadline = Instant::now() + WAIT;
    let private = loop {
        if let Some(guard) = Store::try_lock(&scope.private_dir, false).map_err(fail)? {
            break guard;
        }
        if Instant::now() >= deadline {
            return Err(fail("the native mobile authorization lock stayed busy"));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Profile::load_locked(&private)
        .map_err(fail)?
        .check_configuration(&scope.configuration, &scope.private_dir)
        .map_err(fail)?;
    let verified = authorize(&private, envelope, &scope.machine, now).map_err(fail)?;
    let payload: Exchange = serde_json::from_value(verified.payload().clone())
        .map_err(|_| fail("invalid authenticated edit exchange"))?;
    if payload.schema != 1
        || payload.updates.len() != 1
        || payload.inventory != Inventory::default()
    {
        return Err(fail(
            "an authenticated edit must be one source-bound opaque frame",
        ));
    }
    let update = &payload.updates[0];
    if update.bytes.len() > zerus_replica::phone::PAGE.div_ceil(3) * 4 {
        return Err(fail("the mobile edit frame exceeds its page bound"));
    }
    let frame = Frame::decode(
        &STANDARD
            .decode(&update.bytes)
            .map_err(|_| fail("invalid mobile edit base64"))?,
    )
    .map_err(fail)?;
    if payload.project != frame.header.project
        || update.task.as_deref() != Some(frame.header.task.as_str())
    {
        return Err(fail("the mobile edit belongs to another project or task"));
    }
    let native_store = ProjectStore::store_id_until(&scope.replica_root, Instant::now() + WAIT)
        .map_err(fail)?
        .ok_or_else(|| fail("the native replica identity stayed busy"))?;
    let binding = TaskBinding {
        machine_id: scope.machine.clone(),
        native_store: native_store.clone(),
        phone_store: frame.header.command.phone_store().into(),
        project: frame.header.project.clone(),
        task: frame.header.task.clone(),
        edit_id: frame.header.command.edit_id().into(),
        receipt_nonce: frame.header.command.receipt_nonce().map(Into::into),
    };
    let turn = match native_journal::reserve(&verified, binding).map_err(fail)? {
        Admission::Retained(outcome) => return answer(&verified, outcome),
        Admission::Fresh(turn) => turn,
    };
    let started = Cell::new(false);
    let outcome = match perform(
        scope,
        &private,
        &verified,
        &frame,
        &native_store,
        now,
        &started,
    ) {
        Ok(result) => Outcome::Completed { result },
        Err(_) if started.get() => Outcome::Unknown,
        Err(error) => Outcome::Refused {
            error: error.to_string(),
        },
    };
    turn.finish(outcome.clone()).map_err(fail)?;
    answer(&verified, outcome)
}

fn answer(request: &VerifiedRequest<'_>, outcome: Outcome) -> Result<Value> {
    let mut value = serde_json::to_value(outcome).map_err(fail)?;
    value["schema"] = json!(1);
    value["kind"] = json!("mobile_task_edit");
    value["request_id"] = json!(request.request_id());
    value["envelope_digest"] = json!(request.envelope_digest());
    if value["state"] == "unknown" {
        value["error"]=json!("The original mobile edit has no durable completion; its outcome is unknown and it will not execute again. Keep the draft and sync this task for review");
    }
    Ok(value)
}

fn device_key(device: &DevicePublic) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(device.canonical_bytes().map_err(fail)?)
    ))
}

fn perform(
    scope: &Endpoint,
    private: &Store,
    request: &VerifiedRequest<'_>,
    frame: &Frame,
    native_store: &str,
    now: u64,
    started: &Cell<bool>,
) -> Result<Value> {
    let store = ProjectStore::open_within(&scope.replica_root, &frame.header.project, WAIT)
        .map_err(fail)?
        .ok_or_else(|| fail("the task's project stayed busy"))?;
    let mut admitted = store
        .admit_until(Instant::now() + WAIT)
        .ok_or_else(|| fail("the task's native admission stayed busy"))?;
    let current = admitted.load_complete(&frame.header.task).map_err(fail)?;
    admitted.cutover_ready(&current).map_err(fail)?;
    if current.schema() != zerus_replica::TASK_SCHEMA || !current.view().map_err(fail)?.writable {
        return Err(fail("Update Zerus to edit this task"));
    }
    let current_store = ProjectStore::store_id_until(&scope.replica_root, Instant::now() + WAIT)
        .map_err(fail)?
        .ok_or_else(|| fail("the native replica identity stayed busy"))?;
    if current_store != native_store {
        return Err(fail("the native replica store changed; the draft was kept"));
    }
    let baseline = Baseline::capture(&current).map_err(fail)?;
    match &frame.header.command {
        Command::Qualify {
            phone_store,
            edit_id,
            seal,
            version,
        } => {
            if edit_id == request.request_id()
                || *seal != baseline.seal
                || !zerus_replica::version_includes(&baseline.version, version)
            {
                return Err(fail("This task has unsynchronized local changes; your draft was kept. Review it on its computer"));
            }
            let receipt = Receipt {
                version: 1,
                workspace: request.workspace().into(),
                phone_device: request.source().device_id.clone(),
                phone_key_generation: request.source().key_generation,
                phone_key: device_key(request.source())?,
                host_device: request.host().device_id.clone(),
                host_key_generation: request.host().key_generation,
                host_key: device_key(request.host())?,
                machine_id: scope.machine.clone(),
                native_store: native_store.into(),
                phone_store: phone_store.clone(),
                qualification_id: request.request_id().into(),
                edit_id: edit_id.clone(),
                nonce: zerus_replica::ids::new_uuid(),
                baseline,
            };
            let bytes = serde_json::to_vec(&receipt).map_err(fail)?;
            started.set(true);
            private
                .replace(&format!("mobile-permit-{}.json", receipt.nonce), "", &bytes)
                .map_err(fail)?;
            Ok(json!({"receipt":receipt,"task":current.uuid()}))
        }
        Command::Apply {
            phone_store,
            edit_id,
            receipt_nonce,
        }
        | Command::Upload {
            phone_store,
            edit_id,
            receipt_nonce,
        } => {
            let upload = matches!(frame.header.command, Command::Upload { .. })
                .then(|| frame.upload_ack())
                .transpose()?;
            // Intermediate parts have their own durable UUIDs. Only the final
            // complete frame may use the edit UUID retained at qualification.
            if upload.as_ref().is_none_or(|ack| ack.complete) != (edit_id == request.request_id()) {
                return Err(fail(
                    "this receipt belongs to a different original edit UUID",
                ));
            }
            let bytes = private
                .read(&format!("mobile-permit-{receipt_nonce}.json"))
                .map_err(fail)?
                .ok_or_else(|| {
                    fail("the original mobile edit receipt is unavailable; keep the draft")
                })?;
            let receipt: Receipt = serde_json::from_slice(&bytes)
                .map_err(|_| fail("the retained mobile edit receipt is invalid"))?;
            if receipt.version != 1
                || receipt.nonce != *receipt_nonce
                || receipt.edit_id != *edit_id
                || receipt.phone_store != *phone_store
                || receipt.workspace != request.workspace()
                || receipt.phone_device != request.source().device_id
                || receipt.phone_key_generation != request.source().key_generation
                || receipt.phone_key != device_key(request.source())?
                || receipt.host_device != request.host().device_id
                || receipt.host_key_generation != request.host().key_generation
                || receipt.host_key != device_key(request.host())?
                || receipt.machine_id != scope.machine
                || receipt.native_store != native_store
                || receipt.baseline.seal != baseline.seal
                || receipt.baseline.project != frame.header.project
                || receipt.baseline.task != frame.header.task
            {
                return Err(fail("the original mobile receipt no longer matches its source, task or admitted store; the draft was kept"));
            }
            let body = if let Some(expected) = &upload {
                started.set(true);
                let (ack, completed) = zerus_replica::mobile_edit::wire::assemble(
                    &store,
                    &frame.header.task,
                    frame.body.clone(),
                )?;
                // Assembly settled without a canonical save. A later semantic
                // refusal is known to preserve the task; an assembly I/O error
                // before here remains Unknown for this exact part UUID.
                started.set(false);
                if ack != *expected {
                    return Err(fail("the assembled edit acknowledgement changed"));
                }
                let Some(body) = completed else {
                    return Ok(json!({"task":current.uuid(),"upload":ack}));
                };
                body
            } else {
                frame.body.clone()
            };
            let manifest = frame.manifest(&body)?;
            let mut candidate = manifest
                .candidate(
                    &current,
                    &receipt.baseline,
                    i64::try_from(now).map_err(fail)?.saturating_mul(1000),
                )
                .map_err(fail)?;
            started.set(true);
            admitted.save(&mut candidate).map_err(fail)?;
            #[cfg(test)]
            testing::after_save();
            let mut result = json!({"task":candidate.uuid(),"edited":true,"version":candidate.applied_version()});
            if let Some(ack) = upload {
                result["upload"] = serde_json::to_value(ack).map_err(fail)?;
            }
            Ok(result)
        }
    }
}

#[cfg(test)]
mod testing {
    thread_local! { pub(super) static EXIT_AFTER_SAVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    pub(super) fn after_save() {
        if EXIT_AFTER_SAVE.with(std::cell::Cell::get) {
            // A fixture child exits abruptly: no unwinding, no result record.
            std::process::exit(73);
        }
    }
}

#[cfg(test)]
mod tests;
