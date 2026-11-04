//! Displayed original selection only. Neither this view nor its nonce is
//! caller, Grant, cleanup or manager authority. Effects use fresh native checks.
use super::*;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Disposition {
    Required,
    RebootRequired,
    ReviewedUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct View {
    pub request_id: String,
    pub configuration: String,
    pub selection: String,
    pub disposition: Disposition,
    pub passive_available: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    Review,
    Background,
    Passive,
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
/// Single-use, native-confirmed local continuation. No JSON, receipt, PID or
/// renderer-provided Boolean can construct or clone it. Every consuming native
/// helper still acquires its own caller/current Grant/registration proofs.
pub(in crate::desktop_service) struct Prepared {
    display: View,
    action: Action,
    route: Arc<Route>,
    resources: Resources,
    _busy: Busy,
}
impl Prepared {
    pub(in crate::desktop_service) fn action(&self) -> Action {
        self.action
    }
    pub(in crate::desktop_service) fn background(&self) -> bool {
        self.action == Action::Background
    }
    pub(in crate::desktop_service) fn configuration(&self) -> &str {
        &self.display.configuration
    }
}

pub(super) struct Target {
    pub view: View,
    pub route: Arc<Route>,
    anchor: Value,
}

struct Frame {
    request: String,
    disposition: Disposition,
    anchor: Value,
}
fn id(value: &Value) -> Result<String, &'static str> {
    value
        .as_str()
        .filter(|s| uuid::Uuid::parse_str(s).is_ok_and(|u| !u.is_nil() && u.to_string() == *s))
        .map(str::to_owned)
        .ok_or("activation_review_unavailable")
}
fn state(value: &Value) -> Result<Disposition, &'static str> {
    match value.as_str() {
        Some("reboot_required") => Ok(Disposition::RebootRequired),
        Some("reviewed_unknown") => Ok(Disposition::ReviewedUnknown),
        _ => Err("activation_review_unavailable"),
    }
}
impl Frame {
    fn read(status: &Value, windows: bool) -> Result<Option<Self>, &'static str> {
        if windows {
            let pending = &status["pending_transition"];
            let archived = &status["reviewed_transition"];
            if pending.is_object() {
                let request = id(&pending["request_id"])?;
                let disposition = if pending["review"].is_null() {
                    Disposition::Required
                } else {
                    state(&pending["review"]["state"])?
                };
                if disposition == Disposition::ReviewedUnknown {
                    return Err("activation_review_unavailable");
                }
                return Ok(Some(Self {
                    request,
                    disposition,
                    anchor: json!([
                        status["task_name"],
                        status["selected_mode"],
                        status["mode_choice"],
                        status["service_epoch"],
                        status["service_generation"],
                        pending
                    ]),
                }));
            }
            if archived.is_object() {
                let request = id(&archived["original"]["request_id"])?;
                if archived["outcome"] != "unknown"
                    || state(&archived["original"]["review"]["state"])?
                        != Disposition::ReviewedUnknown
                {
                    return Err("activation_review_unavailable");
                }
                return Ok(Some(Self {
                    request,
                    disposition: Disposition::ReviewedUnknown,
                    anchor: json!([
                        status["task_name"],
                        status["selected_mode"],
                        status["mode_choice"],
                        status["service_epoch"],
                        status["service_generation"],
                        archived
                    ]),
                }));
            }
        } else {
            let reviewed = &status["activation_review"];
            let pending = &status["activation_pending"];
            if !pending.is_null() {
                let request = id(pending)?;
                let disposition = if reviewed["request_id"] == request {
                    if reviewed["phase"] != "unknown"
                        || !reviewed["result"].is_null()
                        || reviewed["disposition"]["request_id"] != request
                        || reviewed["disposition"]["binding"]["configuration"]
                            != status["configuration"]
                    {
                        return Err("activation_review_unavailable");
                    }
                    state(&reviewed["disposition"]["state"])?
                } else {
                    Disposition::Required
                };
                if disposition == Disposition::ReviewedUnknown {
                    return Err("activation_review_unavailable");
                }
                return Ok(Some(Self {
                    request,
                    disposition,
                    anchor: json!([
                        status["selected_mode"],
                        status["mode_selection"],
                        status["transition"],
                        status["mode_choice"],
                        status["service_epoch"],
                        status["service_generation"],
                        pending,
                        reviewed
                    ]),
                }));
            }
            if reviewed.is_object() {
                let request = id(&reviewed["request_id"])?;
                let disposition = state(&reviewed["disposition"]["state"])?;
                if disposition != Disposition::ReviewedUnknown
                    || reviewed["phase"] != "unknown"
                    || !reviewed["result"].is_null()
                    || reviewed["disposition"]["request_id"] != request
                    || reviewed["disposition"]["binding"]["configuration"]
                        != status["configuration"]
                {
                    return Err("activation_review_unavailable");
                }
                return Ok(Some(Self {
                    request,
                    disposition,
                    anchor: json!([
                        status["selected_mode"],
                        status["mode_selection"],
                        status["transition"],
                        status["mode_choice"],
                        status["service_epoch"],
                        status["service_generation"],
                        reviewed
                    ]),
                }));
            }
        }
        if status["activation_automatic_blocked"] == true {
            return Err("activation_review_unavailable");
        }
        Ok(None)
    }
}

impl Controller {
    pub(super) fn record_review(
        &self,
        route: Arc<Route>,
        configuration: &str,
        status: &Value,
        windows: bool,
    ) -> Result<(), &'static str> {
        if status["configuration"] != configuration
            || !zerus_client_core::desktop::digest(configuration)
        {
            return Err("activation_scope_changed");
        }
        let frame = Frame::read(status, windows)?;
        let previous = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .review
            .as_ref()
            .map(|r| r.route.clone());
        if let Some(previous) = previous {
            previous.check()?;
        }
        let mut current = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        super::latch(&mut current, status);
        let Some(frame) = frame else {
            if current.view.automatic_blocked {
                return Err("activation_review_unavailable");
            }
            current.review = None;
            current.view.review = None;
            return Ok(());
        };
        if let Some(original) = &current.review {
            if original.view.configuration != configuration
                || original.route.cli != route.cli
                || original.route.helper != route.helper
            {
                return Err("activation_scope_changed");
            }
        }
        let unchanged = current.review.as_ref().is_some_and(|r| {
            r.view.request_id == frame.request
                && r.view.disposition == frame.disposition
                && r.anchor == frame.anchor
        });
        let view = if unchanged {
            current.review.as_ref().expect("original").view.clone()
        } else {
            View {
                request_id: frame.request,
                configuration: configuration.into(),
                selection: uuid::Uuid::new_v4().to_string(),
                disposition: frame.disposition,
                passive_available: route.windows,
            }
        };
        let route = current
            .review
            .as_ref()
            .map(|r| r.route.clone())
            .unwrap_or(route);
        current.review = Some(Target {
            view: view.clone(),
            route,
            anchor: frame.anchor,
        });
        current.view.review = Some(view);
        current.view.needs_resume = false;
        Ok(())
    }

    pub(in crate::desktop_service) fn blocks_resume(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.view.automatic_blocked || state.view.review.is_some() || state.view.pending
    }

    fn checked_target(&self, display: &View, action: Action) -> Result<Arc<Route>, &'static str> {
        if self.stop.load(Ordering::Acquire) {
            return Err("stopped");
        }
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let target = state
            .review
            .as_ref()
            .ok_or("activation_review_unavailable")?;
        if &target.view != display {
            return Err("activation_scope_changed");
        }
        if action == Action::Review {
            if display.disposition == Disposition::ReviewedUnknown {
                return Err("activation_already_reviewed");
            }
        } else {
            if !state.choice_capable {
                return Err("activation_helper_update_required");
            }
            if display.disposition != Disposition::ReviewedUnknown
                || (action == Action::Passive && !target.route.windows)
            {
                return Err("activation_separate_choice_required");
            }
            if state
                .intent
                .as_ref()
                .is_some_and(|i| !i.complete && i.id != display.request_id)
            {
                return Err("activation_unconfirmed");
            }
        }
        let route = target.route.clone();
        drop(state);
        route.check()?;
        Ok(route)
    }

    pub(in crate::desktop_service) async fn confirm(
        self: &Arc<Self>,
        display: View,
        action: Action,
        confirmation: impl FnOnce(&View, Action) -> bool + Send + 'static,
    ) -> Result<Option<Prepared>, &'static str> {
        let charge = self
            .gate
            .admit(Role::Fleet)
            .map_err(|_| "activation_original_resources_pending")?;
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let route = owner.checked_target(&display, action)?;
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
            owner.checked_target(&display, action)?;
            if !confirmation(&display, action) {
                return Ok(None);
            }
            // The native dialog never freezes the external registration. A
            // changed original, helper or selection invalidates that consent.
            owner
                .status(route.clone(), &display.configuration, resources.clone())
                .ok_or("activation_status_unavailable")?;
            owner.checked_target(&display, action)?;
            Ok(Some(Prepared {
                display,
                action,
                route,
                resources,
                _busy: busy,
            }))
        })
        .await
        .map_err(|_| "activation_confirmation_unavailable")?
    }

    pub(in crate::desktop_service) async fn apply(
        self: &Arc<Self>,
        original: Prepared,
    ) -> Option<String> {
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let Prepared {
                display,
                action,
                route,
                resources,
                _busy,
            } = original;
            if owner
                .status(route.clone(), &display.configuration, resources.clone())
                .is_none()
                || owner.checked_target(&display, action).is_err()
            {
                owner.failed("activation_scope_changed");
                return None;
            }
            if action == Action::Review {
                // No Grant/Rearm, activation, new UUID or manager call here.
                // Even a lost reply is followed only by an original-bound read.
                owner.call(
                    route.clone(),
                    route.review_arguments(&display.request_id),
                    resources.clone(),
                );
                owner.status(route, &display.configuration, resources);
                None
            } else {
                let id = uuid::Uuid::new_v4().to_string();
                owner.reserve(id.clone(), display.configuration.clone(), route.clone());
                owner.call_effect(
                    route.clone(),
                    route.choice_arguments(&display.request_id, &id, action),
                    resources.clone(),
                    &id,
                );
                owner.lookup(
                    id.clone(),
                    display.configuration.clone(),
                    route.clone(),
                    resources.clone(),
                );
                owner.status(route, &display.configuration, resources);
                Some(id)
            }
        })
        .await
        .ok()
        .flatten()
    }
    /// A read-only veto derived only from this native-confirmed original.
    /// It retains the exact route/resources through any ownership child loan.
    pub(in crate::desktop_service) fn effect_check(
        self: &Arc<Self>,
        original: &Prepared,
    ) -> crate::desktop_ownership::EffectCheck {
        let owner = self.clone();
        let display = original.display.clone();
        let action = original.action;
        let route = original.route.clone();
        let resources = original.resources.clone();
        Arc::new(move || {
            owner.checked_target(&display, action)?;
            owner
                .status(route.clone(), &display.configuration, resources.clone())
                .ok_or("activation_status_unavailable")?;
            owner.checked_target(&display, action).map(|_| ())
        })
    }
    pub(in crate::desktop_service) async fn validate(
        self: &Arc<Self>,
        original: &Prepared,
    ) -> Result<(), &'static str> {
        let owner = self.clone();
        let display = original.display.clone();
        let action = original.action;
        let route = original.route.clone();
        let resources = original.resources.clone();
        tauri::async_runtime::spawn_blocking(move || {
            owner
                .status(route, &display.configuration, resources)
                .ok_or("activation_status_unavailable")?;
            owner.checked_target(&display, action).map(|_| ())
        })
        .await
        .map_err(|_| "activation_status_unavailable")?
    }
}

/// Read-only routing evidence. It cannot construct a Grant, trigger a manager
/// operation, or admit any per-kind import. Those retain their own native gates.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    journal: String,
    owner_epoch: String,
    service_epoch: String,
    generation: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    reviewed: String,
    request: String,
    desired: String,
    phase: String,
    scope: Scope,
}
impl Choice {
    fn current(
        status: &Value,
        reviewed: &str,
        configuration: &str,
        ownership: &zerus_client_core::desktop::ownership::Status,
        epoch: &str,
        generation: u64,
    ) -> Option<Self> {
        use zerus_client_core::desktop::ownership::State;
        let choice: Self = serde_json::from_value(status["mode_choice"].clone()).ok()?;
        let valid_id = |s: &str| id(&Value::String(s.into())).is_ok();
        (status["configuration"] == configuration
            && status["selected_mode"] == "background"
            && status["running_mode"] == "background"
            && status["ipc_ready"] == true
            && status["pending_restart"] == false
            && status["activation_pending"].is_null()
            && status["pending_transition"].is_null()
            && status["transition"].is_null()
            && choice.reviewed == reviewed
            && choice.request != reviewed
            && valid_id(&choice.reviewed)
            && valid_id(&choice.request)
            && choice.desired == "background"
            && choice.phase == "settled"
            && ownership.matches(configuration)
            && ownership.state == State::Service
            && ownership.identity_current
            && !ownership.rearm_required
            && ownership.journal.as_deref() == Some(choice.scope.journal.as_str())
            && ownership.owner_epoch.as_deref() == Some(choice.scope.owner_epoch.as_str())
            && valid_id(&choice.scope.journal)
            && valid_id(&choice.scope.owner_epoch)
            && valid_id(epoch)
            && choice.scope.service_epoch == epoch
            && choice.scope.generation == generation
            && generation < (1u64 << 53)
            && status["service_epoch"] == epoch
            && status["service_generation"] == generation)
            .then_some(choice)
    }
    fn original(&self, result: &Value, windows: bool, configuration: &str) -> bool {
        if windows {
            result["abi"] == "desktop-registration-mode-v1"
                && result["configuration"] == configuration
                && result["request_id"] == self.request
                && result["read_only"] == true
                && serde_json::from_value::<Self>(result["receipt"].clone())
                    .ok()
                    .as_ref()
                    == Some(self)
        } else {
            result["abi"] == ABI
                && result["configuration"] == configuration
                && result["request_id"] == self.request
                && result["read_only"] == true
                && result["phase"] == "settled"
                && result["result"] == json!({"ok":true,"code":"ready"})
                && result["after_review"] == self.reviewed
                && serde_json::from_value::<Scope>(result["scope"].clone())
                    .ok()
                    .as_ref()
                    == Some(&self.scope)
        }
    }
}
pub(super) struct Business {
    choice: Choice,
    configuration: String,
}
impl Controller {
    pub(in crate::desktop_service) fn business_permitted(
        &self,
        snapshot: &service::Snapshot,
        ownership: Option<&zerus_client_core::desktop::ownership::Status>,
    ) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(business) = &state.business else {
            return false;
        };
        let Some(ownership) = ownership else {
            return false;
        };
        let Some(status) = &state.selection else {
            return false;
        };
        !state.view.pending
            && !state.view.running
            && snapshot.connected
            && snapshot.epoch.as_deref() == Some(business.choice.scope.service_epoch.as_str())
            && snapshot.generation == Some(business.choice.scope.generation)
            && Choice::current(
                status,
                &business.choice.reviewed,
                &business.configuration,
                ownership,
                &business.choice.scope.service_epoch,
                business.choice.scope.generation,
            )
            .as_ref()
                == Some(&business.choice)
    }
    pub(in crate::desktop_service) async fn business_allowed(
        self: &Arc<Self>,
        grant: &crate::desktop_ownership::CurrentGrant,
        epoch: String,
        generation: u64,
        offered: String,
    ) -> bool {
        let charge = match self.gate.admit(Role::Fleet) {
            Ok(c) => c,
            Err(_) => return false,
        };
        let original = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .review
                .as_ref()
                .filter(|r| {
                    r.view.disposition == Disposition::ReviewedUnknown
                        && state.choice_capable
                        && !state.view.pending
                        && state
                            .intent
                            .as_ref()
                            .is_some_and(|i| i.id == offered && i.complete && i.joined)
                })
                .map(|r| (r.route.clone(), r.view.clone()))
        };
        let Some((route, display)) = original else {
            return false;
        };
        let ownership = grant.status().clone();
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let resources: Resources = Arc::new(charge);
            let before = owner.status(route.clone(), &display.configuration, resources.clone())?;
            let choice = Choice::current(
                &before,
                &display.request_id,
                &display.configuration,
                &ownership,
                &epoch,
                generation,
            )?;
            // A previous healthy background receipt cannot settle THIS new
            // confirmation when its original was refused/missing or never offered.
            if choice.request != offered {
                return None;
            }
            let bytes = owner.call_bounded(
                route.clone(),
                route.arguments(Some(&choice.request), false, false),
                resources.clone(),
                Duration::from_secs(3),
            )?;
            let result: Value = serde_json::from_slice(&bytes).ok()?;
            if !choice.original(&result, route.windows, &display.configuration) {
                return None;
            }
            let after = owner.status(route, &display.configuration, resources)?;
            if Choice::current(
                &after,
                &display.request_id,
                &display.configuration,
                &ownership,
                &epoch,
                generation,
            )
            .as_ref()
                != Some(&choice)
            {
                return None;
            }
            owner
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .business = Some(Business {
                choice,
                configuration: display.configuration,
            });
            Some(())
        })
        .await
        .ok()
        .flatten()
        .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const REQUEST: &str = "11111111-1111-4111-8111-111111111111";
    const CONFIG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    #[test]
    fn both_platform_frames_keep_unknown_separate_from_readiness_and_review() {
        let pending = json!({"request_id":REQUEST,"original":{"pid":1,"birth":2},"review":null});
        let windows = json!({"configuration":CONFIG,"pending_transition":pending,"selected_mode":"background","running_mode":"background"});
        let read = Frame::read(&windows, true).unwrap().unwrap();
        assert_eq!(read.disposition, Disposition::Required);
        let mut changed = windows.clone();
        changed["pending_transition"]["original"]["birth"] = 3.into();
        assert_ne!(
            read.anchor,
            Frame::read(&changed, true).unwrap().unwrap().anchor
        );
        let unix = json!({"configuration":CONFIG,"activation_pending":REQUEST,"selected_mode":"background","ipc_ready":true});
        assert_eq!(
            Frame::read(&unix, false).unwrap().unwrap().disposition,
            Disposition::Required
        );
        let inconsistent = json!({"configuration":CONFIG,"activation_pending":REQUEST,"activation_review":{
            "request_id":REQUEST,"phase":"unknown","result":null,"disposition":{"state":"reviewed_unknown"}}});
        assert!(Frame::read(&inconsistent, false).is_err());
        assert!(Frame::read(&json!({"activation_automatic_blocked":true}), true).is_err());
        assert!(Frame::read(&json!({"activation_pending":"unbound"}), false).is_err());
    }
}

#[cfg(test)]
mod business_tests {
    use super::*;
    use zerus_client_core::desktop::ownership::{self, Status};
    const OLD: &str = "11111111-1111-4111-8111-111111111111";
    const NEW: &str = "22222222-2222-4222-8222-222222222222";
    const EPOCH: &str = "33333333-3333-4333-8333-333333333333";
    const CONFIG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    // Routing predicate model only. This Status cannot construct CurrentGrant.
    fn model() -> (Value, Status) {
        let status: Status =
            serde_json::from_value(json!({"abi":ownership::ABI,"configuration":CONFIG,
            "journal":OLD,"revision":7,"state":"service","rearm_required":false,
            "request_id":NEW,"owner_epoch":NEW,"identity_current":true}))
            .unwrap();
        (
            json!({"configuration":CONFIG,"selected_mode":"background","running_mode":"background",
            "ipc_ready":true,"pending_restart":false,"service_epoch":EPOCH,"service_generation":1,
            "mode_choice":{"request":NEW,"reviewed":OLD,"desired":"background","phase":"settled",
                "scope":{"journal":OLD,"owner_epoch":NEW,"service_epoch":EPOCH,"generation":1}}}),
            status,
        )
    }
    #[test]
    fn latest_choice_requires_current_granted_origin_and_the_same_service_scope() {
        let (status, ownership) = model();
        let choice = Choice::current(&status, OLD, CONFIG, &ownership, EPOCH, 1).unwrap();
        let result = json!({"abi":ABI,"configuration":CONFIG,"request_id":NEW,"read_only":true,
            "after_review":OLD,"phase":"settled","result":{"ok":true,"code":"ready"},
            "scope":status["mode_choice"]["scope"]});
        assert!(choice.original(&result, false, CONFIG));
        let windows = json!({"abi":"desktop-registration-mode-v1","configuration":CONFIG,
            "request_id":NEW,"read_only":true,"receipt":status["mode_choice"]});
        assert!(choice.original(&windows, true, CONFIG));
        let mut wrong = windows.clone();
        wrong["receipt"]["request"] = OLD.into();
        assert!(!choice.original(&wrong, true, CONFIG));
        for (key, value) in [
            ("phase", json!("unknown")),
            ("after_review", json!(NEW)),
            ("read_only", json!(false)),
            ("scope", Value::Null),
        ] {
            let mut bad = result.clone();
            bad[key] = value;
            assert!(!choice.original(&bad, false, CONFIG), "{key}");
        }
        for (pointer, value) in [
            ("/selected_mode", json!("passive")),
            ("/running_mode", json!("passive")),
            ("/activation_pending", json!(NEW)),
            ("/mode_choice/phase", json!("unknown")),
            ("/mode_choice/desired", json!("passive")),
            ("/mode_choice/reviewed", json!(NEW)),
            ("/mode_choice/scope/owner_epoch", json!(OLD)),
            ("/service_epoch", json!(NEW)),
            ("/service_generation", json!(2)),
            ("/pending_restart", json!(true)),
        ] {
            let mut bad = status.clone();
            if pointer == "/activation_pending" {
                bad["activation_pending"] = value;
            } else {
                *bad.pointer_mut(pointer).unwrap() = value;
            }
            assert!(
                Choice::current(&bad, OLD, CONFIG, &ownership, EPOCH, 1).is_none(),
                "{pointer}"
            );
        }
        for change in 0..4 {
            let mut bad = ownership.clone();
            match change {
                0 => bad.identity_current = false,
                1 => bad.owner_epoch = Some(OLD.into()),
                2 => bad.rearm_required = true,
                _ => bad.state = ownership::State::Revoked,
            };
            assert!(Choice::current(&status, OLD, CONFIG, &bad, EPOCH, 1).is_none());
        }
        assert!(Choice::current(&status, OLD, CONFIG, &ownership, NEW, 1).is_none());
    }
}
