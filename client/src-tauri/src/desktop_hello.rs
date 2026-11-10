//! Background setup for the canonical instance's read-only delegation hello.
//! The socket callback uses cached identity only; there is no probe-time child.
use std::sync::{Arc, Mutex, PoisonError};
#[cfg(unix)]
use zerus_client_core::desktop;
use zerus_client_core::desktop::{Build, Responder};

/// The callback follows one native startup transition, never a wire request.
/// Recovery installs a NEW responder/instance; existing evidence is immutable.
pub(crate) struct Canonical(Arc<Mutex<Arc<Responder>>>);
impl Canonical {
    pub(crate) fn recover(
        &self,
        identity: &crate::identity::Identity,
        original: &crate::desktop_ownership::NativeRecovery,
    ) -> Arc<Responder> {
        crate::desktop_service::enter_startup_recovery();
        // Task 180: the app shows which check refused and the next step.
        // Task #53: a missing binding says why the install recorded none.
        crate::desktop_service::record_startup_recovery_reason(crate::takeover_refusal::refined(original.reason));
        let responder = responder(identity);
        self.replace(responder)
    }
    fn replace(&self, responder: Arc<Responder>) -> Arc<Responder> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = responder.clone();
        responder
    }
    fn reply(&self, line: &[u8]) -> Option<Vec<u8>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reply(line)
    }
}

fn responder(identity: &crate::identity::Identity) -> Arc<Responder> {
    let info = crate::cli::build_info(&crate::cli::BUILD);
    let responder = Arc::new(Responder::new(
        identity.bundle_id.clone(),
        Build {
            version: info["versionName"].as_str().unwrap_or("").into(),
            source_commit: info["sourceCommit"].as_str().unwrap_or("").into(),
            source_dirty: info["sourceDirty"].as_bool(),
        },
        uuid::Uuid::new_v4().to_string(),
        true,
        std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|v| !v.is_empty()),
    ));
    // Native startup decides once, before canonical claim. Only ownership
    // original-child evidence carries this binding; ordinary Hello is unchanged.
    let mode = if crate::desktop_service::mode() {
        zerus_client_core::desktop::ownership::Mode::Service
    } else {
        zerus_client_core::desktop::ownership::Mode::Native
    };
    responder.configure_ownership_mode_once(mode);
    responder
}

pub(crate) fn prepare(
    identity: &crate::identity::Identity,
) -> (crate::instance::Reply, Arc<Responder>, Canonical) {
    let responder = responder(identity);
    let canonical = Canonical(Arc::new(Mutex::new(responder.clone())));
    let cached = Canonical(canonical.0.clone());
    let reply: crate::instance::Reply = Arc::new(move |stream, line| {
        #[cfg(unix)]
        let same = desktop::same_user(stream);
        #[cfg(windows)]
        let same = crate::instance::same_user(stream);
        same.then(|| cached.reply(line)).flatten()
    });
    (reply, responder, canonical)
}

/// Only the process that claimed the canonical socket calls this, before any
/// writers or GUI actor exist. Replay and other bundle identities stay isolated.
#[cfg(unix)]
pub(crate) fn hold(
    production: bool,
    responder: &Arc<Responder>,
) -> Result<
    (
        Option<desktop::startup::Bootstrap>,
        Option<crate::desktop_ownership::NativeReady>,
    ),
    HoldRefusal,
> {
    if crate::desktop_service::mode() {
        // A consumer keeps its canonical identity but never claims GUI writer startup.
        return Ok((None, None));
    }
    if !production || std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|v| !v.is_empty()) {
        return Ok((None, None));
    }
    let ownership = crate::desktop_ownership::native_takeover_recoverable(
        zerus_client_core::default_hgs(),
        responder.clone(),
    )
    .map_err(HoldRefusal::Takeover)?;
    let mut held = desktop::startup::Bootstrap::begin(
        ownership.helper().map_err(|code| {
            crate::startup_error::record("native_takeover.helper", code);
            HoldRefusal::Bootstrap
        })?,
        &uuid::Uuid::new_v4().to_string(),
        &[],
    )
    .inspect_err(|&error| {
        bootstrap_refusal("bootstrap.begin", error);
    })
    .map_err(|_| HoldRefusal::Bootstrap)?;
    match held.wait_ready() {
        Ok(()) => Ok((Some(held), Some(ownership))),
        Err(desktop::startup::Refused::Unsupported) => {
            // Compatibility only: preserve today's GUI writers and cached hello,
            // without invoking the unknown command or claiming a held bootstrap.
            let responder = responder.clone();
            let _ = std::thread::Builder::new()
                .name("zerus-desktop-identity".into())
                .spawn(move || {
                    let selected = zerus_client_core::default_hgs();
                    if let Some(configuration) = desktop::selected_configuration(&selected, &[]) {
                        responder.configure_once(configuration);
                    }
                });
            Ok((None, Some(ownership)))
        }
        Err(error) => {
            bootstrap_refusal("bootstrap.ready", error);
            Err(HoldRefusal::Bootstrap)
        }
    }
}

#[cfg(unix)]
pub(crate) enum HoldRefusal {
    Takeover(crate::desktop_ownership::NativeRecovery),
    Bootstrap,
}

#[cfg(unix)]
fn bootstrap_refusal(step: &'static str, error: desktop::startup::Refused) {
    use desktop::startup::Refused;
    crate::startup_error::record(
        step,
        match error {
            Refused::Unsupported => "unsupported",
            Refused::Invalid => "invalid",
            Refused::Unavailable => "unavailable",
            Refused::TimedOut => "timed_out",
            Refused::Unknown => "unknown",
        },
    );
}

/// Request release after setup registered its writers, then retain and actually
/// join the original helper worker off the GUI actor. No late/unknown result can
/// publish configuration or make this instance eligible for producer binding.
#[cfg(unix)]
pub fn finish(mut held: desktop::startup::Bootstrap, responder: Arc<Responder>) {
    if held.release().is_err() {
        eprintln!(
            "zerus-client: desktop startup release failed; producer identity remains unavailable"
        );
        return;
    }
    tauri::async_runtime::spawn_blocking(move || loop {
        match held.try_finish() {
            Ok(Some(configuration)) => {
                responder.configure_once(configuration);
                break;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(_) => {
                eprintln!("zerus-client: desktop startup helper did not finish; producer identity remains unavailable");
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_client_core::desktop::{
        ownership::{Action, Mode},
        ui_requests::Request,
    };

    #[test]
    fn canonical_recovery_uses_a_new_immutable_mode_and_never_reuses_native_evidence() {
        #[cfg(unix)]
        if crate::test_context::non_agent() {
            return;
        }
        fn responder(mode: Mode) -> Arc<Responder> {
            let value = Arc::new(Responder::new(
                "com.hgdev.hgs-tray".into(),
                Build {
                    version: "1.0.0".into(),
                    source_commit: "a".repeat(40),
                    source_dirty: Some(false),
                },
                uuid::Uuid::new_v4().to_string(),
                true,
                false,
            ));
            assert!(value.configure_ownership_mode_once(mode));
            assert!(value.configure_once("a".repeat(64)));
            value
        }
        fn request(action: Action) -> Request {
            Request::DesktopOwnership {
                request_id: uuid::Uuid::new_v4().to_string(),
                action,
                journal: None,
                revision: 0,
            }
        }
        let native = responder(Mode::Native);
        let canonical = Canonical(Arc::new(Mutex::new(native.clone())));
        let nonce = uuid::Uuid::new_v4().to_string();
        let line = format!("desktop-delegation-v1 {nonce}");
        let before = canonical.reply(line.as_bytes()).unwrap();
        let consumer = canonical.replace(responder(Mode::Service));
        let after = canonical.reply(line.as_bytes()).unwrap();
        let before: serde_json::Value = serde_json::from_slice(&before).unwrap();
        let after: serde_json::Value = serde_json::from_slice(&after).unwrap();
        assert_ne!(before["instance_epoch"], after["instance_epoch"]);
        let prior: serde_json::Value =
            serde_json::from_slice(&native.reply(line.as_bytes()).unwrap()).unwrap();
        assert_eq!(prior, before, "old original proof remains immutable");
        assert!(native
            .prepare_ui_request(uuid::Uuid::new_v4().to_string(), request(Action::Rearm))
            .is_err());
        assert!(consumer
            .prepare_ui_request(
                uuid::Uuid::new_v4().to_string(),
                request(Action::NativeTakeover)
            )
            .is_err());
        assert!(consumer
            .prepare_ui_request(uuid::Uuid::new_v4().to_string(), request(Action::Rearm))
            .is_ok());
    }
}
