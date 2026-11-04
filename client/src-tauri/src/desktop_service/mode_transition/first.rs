//! zerus-yfbd.1: the FIRST explicit background choice from a plain passive
//! Windows registration. The displayed offer and its nonce are never consent:
//! only the single-use Prepared made after this native confirmation enters
//! apply, and the native helper repeats every caller, Grant, instance and
//! history check under its own lock. Any interrupted or reviewed history, the
//! automatic latch or a pending original closes the offer here as well.
use super::*;
use serde::Deserialize;

pub(super) const MARKER: &str =
    "Desktop registration first mode choice ABI: desktop-registration-mode-first-choice-v1";

/// What the renderer may show and hand back. Routing only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct View {
    pub configuration: String,
    pub selection: String,
}

pub(super) struct Target {
    view: View,
    route: Arc<Route>,
    anchor: Value,
}

/// The current registration facts one offer stands on: a plain passive
/// selection with no transition, history or latch. Any change in them makes
/// a new selection, so an older displayed offer no longer matches.
pub(super) fn offer(status: &Value) -> Option<Value> {
    (status["selected_mode"] == "passive"
        && status["running_mode"] == "passive"
        && status["ipc_ready"] == true
        && status["pending_restart"] == false
        && status["pending_transition"].is_null()
        && status["transition"].is_null()
        && status["activation_pending"].is_null()
        && status["reviewed_transition"].is_null()
        && status["mode_choice"].is_null()
        && status["activation_automatic_blocked"] == false
        && status["first_choice_lost"].is_null()
        && status["first_choice"]["phase"] != "unknown")
        .then(|| {
            json!([
                status["schema"],
                status["configuration"],
                status["task_name"],
                status["selected_mode"],
                status["running_mode"],
                status["first_choice"],
                status["completed_request_id"],
                status["cancelled_request_id"],
                status["service_epoch"],
                status["service_generation"]
            ])
        })
}

struct Busy(Arc<Controller>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .view
            .running = false;
    }
}

/// Single-use, native-confirmed continuation. No JSON, receipt, PID or
/// renderer Boolean can construct or clone it.
pub(in crate::desktop_service) struct Prepared {
    display: View,
    route: Arc<Route>,
    resources: Resources,
    _busy: Busy,
}
impl Prepared {
    pub(in crate::desktop_service) fn configuration(&self) -> &str {
        &self.display.configuration
    }
}

type Veto = Arc<dyn Fn() -> Result<(), &'static str> + Send + Sync>;

impl Controller {
    /// Read-only: refreshes or closes the offer from one checked status.
    pub(super) fn record_first(&self, route: Arc<Route>, configuration: &str, status: &Value) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let open = route.windows
            && state.first_capable
            && state.review.is_none()
            && !state.view.automatic_blocked
            && !state.view.pending
            && state.intent.as_ref().is_none_or(|i| i.complete)
            && status["configuration"] == configuration;
        let Some(anchor) = open.then(|| offer(status)).flatten() else {
            state.first = None;
            state.view.first = None;
            return;
        };
        let unchanged = state.first.as_ref().is_some_and(|t| {
            t.anchor == anchor
                && t.view.configuration == configuration
                && t.route.cli == route.cli
                && t.route.helper == route.helper
        });
        if !unchanged {
            let view = View {
                configuration: configuration.into(),
                selection: uuid::Uuid::new_v4().to_string(),
            };
            state.view.first = Some(view.clone());
            state.first = Some(Target {
                view,
                route,
                anchor,
            });
        }
    }

    /// `own` admits only this continuation's own reservation, at its spawn.
    pub(super) fn checked_first(
        &self,
        display: &View,
        own: Option<&str>,
    ) -> Result<Arc<Route>, &'static str> {
        if self.stop.load(Ordering::Acquire) {
            return Err("stopped");
        }
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let target = state
            .first
            .as_ref()
            .ok_or("activation_first_choice_unavailable")?;
        if &target.view != display {
            return Err("activation_scope_changed");
        }
        if !state.first_capable {
            return Err("activation_helper_update_required");
        }
        if state.review.is_some() || state.view.automatic_blocked {
            return Err("activation_review_required");
        }
        if let Some(intent) = state.intent.as_ref().filter(|i| !i.complete) {
            if own != Some(intent.id.as_str()) {
                return Err("activation_unconfirmed");
            }
        }
        let route = target.route.clone();
        drop(state);
        route.check()?;
        Ok(route)
    }

    /// At the mutating spawn: a fresh registration read still shows the SAME
    /// offer facts the confirmation stood on. Read-only; refusal prevents it.
    fn spawn_check(
        self: &Arc<Self>,
        route: &Arc<Route>,
        resources: &Resources,
        display: &View,
        own: &str,
    ) -> Result<(), &'static str> {
        self.checked_first(display, Some(own))?;
        let raw = self
            .exchange_bytes(
                route.clone(),
                route.arguments(None, false, false),
                resources.clone(),
                Duration::from_secs(3),
                None,
            )
            .0
            .map_err(|_| "activation_status_unavailable")?;
        let status = route
            .status(serde_json::from_slice(&raw).map_err(|_| "activation_status_unavailable")?);
        // The same checked decoding as every other read; no new selection.
        if !valid_status(&status, &display.configuration) {
            return Err("activation_status_unavailable");
        }
        let fresh = offer(&status).ok_or("activation_scope_changed")?;
        {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let target = state
                .first
                .as_ref()
                .ok_or("activation_first_choice_unavailable")?;
            if target.anchor != fresh || status["configuration"] != display.configuration {
                return Err("activation_scope_changed");
            }
        }
        self.checked_first(display, Some(own)).map(|_| ())
    }

    pub(in crate::desktop_service) async fn confirm_first(
        self: &Arc<Self>,
        display: View,
        confirmation: impl FnOnce(&View) -> bool + Send + 'static,
    ) -> Result<Option<Prepared>, &'static str> {
        let charge = self
            .gate
            .admit(Role::Fleet)
            .map_err(|_| "activation_original_resources_pending")?;
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let route = owner.checked_first(&display, None)?;
            let pin = RoutePin::new(route.clone())?;
            let resources: Resources = Arc::new((charge, pin));
            owner
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .view
                .running = true;
            let busy = Busy(owner.clone());
            owner
                .status(route.clone(), &display.configuration, resources.clone())
                .ok_or("activation_status_unavailable")?;
            owner.checked_first(&display, None)?;
            if !confirmation(&display) {
                return Ok(None);
            }
            // The dialog never freezes the registration: a changed selection,
            // history, latch or helper invalidates that consent.
            owner
                .status(route.clone(), &display.configuration, resources.clone())
                .ok_or("activation_status_unavailable")?;
            owner.checked_first(&display, None)?;
            Ok(Some(Prepared {
                display,
                route,
                resources,
                _busy: busy,
            }))
        })
        .await
        .map_err(|_| "activation_confirmation_unavailable")?
    }

    pub(in crate::desktop_service) async fn validate_first(
        self: &Arc<Self>,
        original: &Prepared,
    ) -> Result<(), &'static str> {
        let owner = self.clone();
        let display = original.display.clone();
        let route = original.route.clone();
        let resources = original.resources.clone();
        tauri::async_runtime::spawn_blocking(move || {
            owner
                .status(route, &display.configuration, resources)
                .ok_or("activation_status_unavailable")?;
            owner.checked_first(&display, None).map(|_| ())
        })
        .await
        .map_err(|_| "activation_status_unavailable")?
    }

    /// Consumes the confirmed continuation. Refusals before the reservation
    /// change nothing; afterwards only the retained original UUID is read.
    pub(in crate::desktop_service) async fn apply_first(
        self: &Arc<Self>,
        original: Prepared,
    ) -> Result<String, &'static str> {
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let Prepared {
                display,
                route,
                resources,
                _busy,
            } = original;
            owner
                .status(route.clone(), &display.configuration, resources.clone())
                .ok_or("activation_status_unavailable")?;
            owner.checked_first(&display, None)?;
            let id = uuid::Uuid::new_v4().to_string();
            owner.reserve(id.clone(), display.configuration.clone(), route.clone());
            if let Some(intent) = owner
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .intent
                .as_mut()
                .filter(|i| i.id == id)
            {
                intent.first = true;
            }
            let veto: Veto = {
                let owner = owner.clone();
                let display = display.clone();
                let id = id.clone();
                let route = route.clone();
                let resources = resources.clone();
                Arc::new(move || owner.spawn_check(&route, &resources, &display, &id))
            };
            owner.call_effect_checked(
                route.clone(),
                route.first_arguments(&id),
                resources.clone(),
                &id,
                Some(veto),
            );
            owner.lookup(
                id.clone(),
                display.configuration.clone(),
                route.clone(),
                resources.clone(),
            );
            owner.status(route, &display.configuration, resources);
            Ok(id)
        })
        .await
        .map_err(|_| "activation_unconfirmed")?
    }

    /// The exact original result of THIS request settled.
    pub(in crate::desktop_service) fn first_settled(&self, id: &str) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .intent
            .as_ref()
            .is_some_and(|i| i.id == id && i.complete)
            && !state.view.pending
            && state.view.code.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(patch: Value) -> Value {
        let mut status = json!({"schema":1,"configuration":"a".repeat(64),"task_name":"hgs-desktop-fixture",
            "selected_mode":"passive","running_mode":"passive","mode_selection":"explicit","ipc_ready":true,
            "pending_restart":false,"pending_transition":null,"transition":null,"activation_pending":null,
            "reviewed_transition":null,"mode_choice":null,"first_choice":null,
            "activation_automatic_blocked":false,"service_epoch":"e","service_generation":1});
        for (key, value) in patch.as_object().unwrap() {
            status[key] = value.clone();
        }
        status
    }

    #[test]
    fn only_a_plain_passive_registration_offers_a_first_choice() {
        assert!(offer(&plain(json!({}))).is_some());
        let refused_before = json!({"request":"11111111-1111-4111-8111-111111111111",
            "desired":"background","phase":"refused"});
        assert!(offer(&plain(json!({"first_choice":refused_before}))).is_some());
        let pending = json!({"request_id":"11111111-1111-4111-8111-111111111111","phase":"held"});
        for (case, patch) in [
            ("background selected", json!({"selected_mode":"background"})),
            ("background running", json!({"running_mode":"background"})),
            (
                "not running",
                json!({"running_mode":null,"ipc_ready":false}),
            ),
            ("pending restart", json!({"pending_restart":true})),
            (
                "pending transition",
                json!({"pending_transition":pending,"transition":pending,
                "activation_pending":"11111111-1111-4111-8111-111111111111"}),
            ),
            (
                "reviewed archive",
                json!({"reviewed_transition":{"outcome":"unknown"}}),
            ),
            (
                "reviewed choice",
                json!({"mode_choice":{"phase":"settled"}}),
            ),
            (
                "automatic latch",
                json!({"activation_automatic_blocked":true}),
            ),
            (
                "older helper without the latch field",
                json!({"activation_automatic_blocked":null}),
            ),
            (
                "unknown first choice",
                json!({"first_choice":{"phase":"unknown"}}),
            ),
            (
                "lost first choice, even unlatched",
                json!({"first_choice_lost":"11111111-1111-4111-8111-111111111111"}),
            ),
        ] {
            assert!(offer(&plain(patch)).is_none(), "{case}");
        }
    }

    #[test]
    fn any_change_in_the_offer_facts_changes_its_anchor() {
        let base = offer(&plain(json!({}))).unwrap();
        for patch in [
            json!({"service_epoch":"other"}),
            json!({"service_generation":2}),
            json!({"task_name":"hgs-desktop-other"}),
            json!({"configuration":"b".repeat(64)}),
            json!({"completed_request_id":"11111111-1111-4111-8111-111111111111"}),
            json!({"cancelled_request_id":"11111111-1111-4111-8111-111111111111"}),
            json!({"first_choice":{"phase":"refused"}}),
        ] {
            assert_ne!(offer(&plain(patch.clone())).unwrap(), base, "{patch}");
        }
    }
}
