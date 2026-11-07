//! One native creation intent, then only scoped reads of its original result.
//! The admitted service job survives this reader and the entire GUI exiting.
use super::audit::help::Image;
use super::*;
use std::time::Instant;
use zerus_client_core::{
    creation,
    desktop::{
        creation_clone as wire, relay,
        ui_requests::{Registration, Request},
    },
    run, Invocation,
};

/// A clone this client never sent: nothing reached the service, so the
/// request ID is retired as definitely effect-free and Continue setup can try
/// again.
pub(crate) const NOT_SENT: &str = zerus_client_core::source!(
    "The repository was not cloned: the request was not sent to the desktop service. Continue setup to try again."
);

/// A definite, effect-free failure for a clone that was never sent, in the
/// shape of a failed clone receipt, so the reducer retires its request ID and
/// the creation stays resumable instead of Unknown.
pub(crate) fn unsent(operation: &Value) -> Option<creation::Answer> {
    let clone = operation["clone_id"].as_str()?;
    if !creation::canonical_uuid(clone) {
        return None;
    }
    let target = operation["target"].as_str()?;
    Some(creation::Answer {
        ok: true,
        data: json!({"status":"failed", "request_id":clone, "path":target,
            "effects_started":false, "detail":NOT_SENT}),
        error: String::new(),
    })
}

pub(super) struct Read {
    pub(super) service: Arc<Service>,
    pub(super) scope: wire::Scope,
    generation: u64,
    cancel: Arc<AtomicBool>,
    routing: Arc<relay::Connection>,
    image: Arc<Image>,
}
// Closed local diagnostics only: no paths, prompts, envelopes or proof material.
fn read_refused(code: &'static str) -> String {
    eprintln!("zerus-client: creation_read_refused code={code}");
    creation::READ_FAILED.into()
}
impl Read {
    pub(super) fn new(service: Option<Arc<Service>>) -> Result<Arc<Self>, String> {
        let service = service.ok_or_else(|| read_refused("no_selected_service"))?;
        let shared = &service.shared;
        let snapshot = service.snapshot();
        if !shared.allowed {
            return Err(read_refused("not_allowed"));
        }
        if !snapshot.connected {
            return Err(read_refused("not_connected"));
        }
        if shared.stopped.load(Ordering::Acquire) {
            return Err(read_refused("stopped"));
        }
        let configuration = shared
            .configuration
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| read_refused("missing_configuration"))?;
        let cancel = shared
            .cancellation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| read_refused("missing_cancellation"))?;
        let routing = shared
            .relay
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| read_refused("missing_relay"))?;
        let helper = routing.executable().map_err(|error| {
            eprintln!("zerus-client: creation_read_refused code=helper_route");
            error.to_owned()
        })?;
        let image = Arc::new(Image::capture(helper).ok_or_else(|| read_refused("helper_image"))?);
        let value = Arc::new(Self {
            service,
            scope: wire::Scope {
                configuration,
                service_epoch: snapshot
                    .epoch
                    .ok_or_else(|| read_refused("missing_epoch"))?,
            },
            generation: snapshot
                .generation
                .ok_or_else(|| read_refused("missing_generation"))?,
            cancel,
            routing,
            image,
        });
        if !value.current() {
            return Err(read_refused("selection_changed"));
        }
        Ok(value)
    }
    pub(super) fn current(&self) -> bool {
        let shared = &self.service.shared;
        let snapshot = self.service.snapshot();
        !self.cancel.load(Ordering::Acquire)
            && !shared.stopped.load(Ordering::Acquire)
            && snapshot.connected
            && snapshot.epoch.as_deref() == Some(self.scope.service_epoch.as_str())
            && snapshot.generation == Some(self.generation)
            && shared
                .configuration
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_deref()
                == Some(self.scope.configuration.as_str())
            && shared
                .cancellation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|v| Arc::ptr_eq(v, &self.cancel))
            && shared
                .relay
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|v| Arc::ptr_eq(v, &self.routing))
    }
    pub(super) fn exchange(
        self: &Arc<Self>,
        request: Option<Request>,
        input: Value,
        help: bool,
        extra: Option<zerus_client_core::desktop::drain::Charge>,
    ) -> Result<Vec<u8>, String> {
        if !self.current() {
            return Err(read_refused("exchange_selection_changed"));
        }
        let rpc = self
            .service
            .shared
            .rpc
            .admit(Role::Fleet)
            .map_err(|error| {
                eprintln!(
                    "zerus-client: creation_read_refused code=rpc_admission refusal={error:?}"
                );
                creation::READ_FAILED
            })?;
        let registration = match request {
            Some(request) => Some(
                self.service
                    .shared
                    .responder
                    .as_ref()
                    .ok_or(creation::READ_FAILED)?
                    .prepare_ui_request(uuid::Uuid::new_v4().to_string(), request)
                    .map_err(|_| creation::READ_FAILED)?,
            ),
            None => None,
        };
        let input = if let Some(registration) = &registration {
            serde_json::to_vec(registration.envelope())
        } else if help {
            Ok(Vec::new())
        } else {
            serde_json::to_vec(&input)
        }
        .map_err(|_| creation::READ_FAILED)?;
        if input.len() > zerus_watch_protocol::CONTROL_LINE_BYTES {
            return Err(creation::READ_FAILED.into());
        }
        let helper = self.routing.executable()?;
        if !self.image.current(helper) {
            return Err(read_refused("exchange_image_changed"));
        }
        let args = if help {
            vec!["--help".into()]
        } else {
            vec![
                "desktop".into(),
                if registration.is_some() {
                    "frontend-request"
                } else {
                    "request"
                }
                .into(),
                "--json".into(),
            ]
        };
        let invocation = Invocation {
            args,
            stdin: Some(input),
            timeout: RPC_WAIT,
            request_id: None,
        };
        let held: Arc<dyn Send + Sync> = Arc::new((
            rpc,
            extra,
            self.clone(),
            registration.as_ref().map(Registration::resources),
        ));
        let result = run::run_owned(
            helper,
            &invocation,
            &[],
            &self.cancel,
            move |command| {
                let child = command.spawn()?;
                if let Some(registration) = registration {
                    let _ = registration.bind_original(&child);
                }
                Ok(child) // Custody is retained even when binding refuses.
            },
            held,
        )
        .map_err(|error| {
            // OwnedError is a closed body-free enum; never print helper stderr.
            eprintln!("zerus-client: creation_read_refused code=helper_exchange refusal={error:?}");
            creation::READ_FAILED
        })?;
        if !self.current()
            || !self.image.current(self.routing.executable()?)
            || result.len() > if help { 64 << 10 } else { wire::REPLY_BYTES }
        {
            return Err(read_refused("exchange_result_changed_or_oversize"));
        }
        Ok(result)
    }
    fn supported(self: &Arc<Self>) -> Result<bool, String> {
        self.supports(wire::HELP_MARKER)
    }
    /// The selected CLI advertises this creation kind and frontend requests.
    pub(super) fn supports(self: &Arc<Self>, kind: &str) -> Result<bool, String> {
        let bytes = self.exchange(None, Value::Null, true, None)?;
        let help = std::str::from_utf8(&bytes).map_err(|_| creation::READ_FAILED)?;
        for marker in [kind, zerus_client_core::desktop::ui_requests::HELP_MARKER] {
            if !help.lines().any(|line| line.trim() == marker) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Called only by the native creation host after its ordered journal barrier.
/// `fresh` is the host's one-use consent for this exact clone (a direct or
/// task Submit, or an explicit Continue's chain), consumed before this call,
/// never from JSON. `wait` marks the clone itself; a status read does not.
///
/// `None` routes the call down the unchanged direct path: only a completed
/// probe that finds no clone support on the selected service does that, for
/// clones and reads alike. The reducer emits a clone only for a request ID it
/// reserved and persisted in this process, so a clone without fresh consent,
/// or one stopped before its admission request is written, was never sent and
/// fails definitely. A missing or Unknown result of a sent original never
/// authorizes a resend or the direct path.
pub(crate) fn call(
    service: Option<Arc<Service>>,
    id: &str,
    operation: &Value,
    fresh: bool,
    wait: bool,
    shutdown: &AtomicBool,
) -> Option<creation::Answer> {
    let sent = std::cell::Cell::new(false);
    let result = (|| -> Result<Option<Value>, String> {
        let read = Read::new(service)?;
        if !creation::valid_operation(id, operation) {
            return Err(creation::READ_FAILED.into());
        }
        // Only a completed probe can prove no support; an I/O error cannot.
        if !read.supported()? {
            return Ok(None);
        }
        if wait && !fresh {
            return Err(creation::READ_FAILED.into());
        }
        let plan = wire::Plan::from_operation(id, operation).ok_or(creation::READ_FAILED)?;
        if fresh {
            let until = Instant::now() + RPC_WAIT;
            let charge = loop {
                if shutdown.load(Ordering::Acquire) || !read.current() || Instant::now() >= until {
                    return Err(creation::READ_FAILED.into());
                }
                match read.service.shared.creation_rpc.admit(Role::Fleet) {
                    Ok(charge) => break charge,
                    Err(zerus_client_core::desktop::drain::Refused::Capacity) => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => return Err(creation::READ_FAILED.into()),
                }
            };
            let intent = wire::Intent {
                scope: read.scope.clone(),
                creation_id: id.into(),
                clone_request_id: plan.clone_request_id.clone(),
                revision: operation["revision"]
                    .as_u64()
                    .ok_or(creation::READ_FAILED)?,
                original: creation::effect_checkpoint(operation)
                    .map_err(|_| creation::READ_FAILED)?,
            };
            intent.clone().verify().map_err(|_| creation::READ_FAILED)?;
            sent.set(true);
            let bytes = read.exchange(
                Some(Request::CreationClone {
                    intent: Box::new(intent.clone()),
                }),
                Value::Null,
                false,
                Some(charge),
            )?;
            let admission: wire::Admission =
                serde_json::from_slice(&bytes).map_err(|_| creation::READ_FAILED)?;
            if !admission.matches(&intent) {
                return Err(creation::READ_FAILED.into());
            }
            if let Some(result) = admission.receipt.and_then(|r| r.result(&plan)) {
                return Ok(Some(result));
            }
        }
        let until = Instant::now() + Duration::from_secs(650);
        let mut delay = Duration::from_millis(250);
        loop {
            if shutdown.load(Ordering::Acquire) || !read.current() {
                return Err(creation::READ_FAILED.into());
            }
            let query = wire::Query::for_plan(read.scope.clone(), id.into(), &plan)
                .ok_or(creation::READ_FAILED)?;
            let bytes = read.exchange(
                None,
                json!({"op":"creation_clone_result","query":query}),
                false,
                None,
            )?;
            let lookup: wire::Lookup =
                serde_json::from_slice(&bytes).map_err(|_| creation::READ_FAILED)?;
            if !lookup.matches(&query) {
                return Err(creation::READ_FAILED.into());
            }
            let receipt = lookup.receipt.ok_or(creation::READ_FAILED)?; // Missing never authorizes Start.
            if let Some(result) = receipt.result(&plan) {
                return Ok(Some(result));
            }
            if !wait || receipt.drained || Instant::now() >= until {
                return Err(creation::READ_FAILED.into());
            }
            let next = Instant::now() + delay;
            while Instant::now() < next {
                if shutdown.load(Ordering::Acquire) || !read.current() {
                    return Err(creation::READ_FAILED.into());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            delay = (delay * 2).min(Duration::from_secs(5));
        }
    })();
    match result {
        Ok(Some(data)) => Some(creation::Answer {
            ok: true,
            data,
            error: String::new(),
        }),
        Ok(None) => None,
        // Nothing reached the service: never Unknown.
        Err(_) => Some(
            (wait && !sent.get())
                .then(|| unsent(operation))
                .flatten()
                .unwrap_or_else(|| creation::Answer {
                    ok: false,
                    data: Value::Null,
                    error: zerus_client_core::repository::UNCONFIRMED_OPERATION.into(),
                }),
        ),
    }
}

#[cfg(all(test, unix))]
mod tests;
