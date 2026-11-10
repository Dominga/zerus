//! Explicit same-mode refresh. Display data selects an original; only the
//! maintained helper's retained Caller/Grant/Running proofs admit its effects.
use super::*;
use serde::Deserialize;

const REFRESH_ABI: &str = "desktop-registration-refresh-v1";
const REFRESH_MARKER: &str = "Desktop registration refresh ABI: desktop-registration-refresh-v1";
const UNAVAILABLE: &str = "refresh_unavailable";
const CHANGED: &str = "refresh_scope_changed";
pub(in crate::desktop_service) type Fence = Arc<dyn Fn() -> Result<(), &'static str> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Offer {
    pub configuration: String,
    pub epoch: String,
    pub selection: String,
}
#[derive(Clone, Default, Serialize)]
pub(crate) struct View {
    pub configuration: Option<String>,
    pub offer: Option<Offer>,
    pub request_id: Option<String>,
    pub running: bool,
    pub settled: bool,
    pub code: String,
}
#[derive(Default)]
pub(super) struct State {
    view: View,
    target: Option<Value>,
    route: Option<Arc<Route>>,
    original: Option<Original>,
}
struct Original {
    id: String,
    configuration: String,
    epoch: String,
    instance: Value,
    scope: Value,
    route: Arc<Route>,
}
/// Neither serializable nor cloneable. It keeps the same route and Fleet
/// charge through the dialog, final veto, original child and real pipe EOFs.
pub(in crate::desktop_service) struct Prepared {
    display: Offer,
    target: Value,
    route: Arc<Route>,
    resources: Resources,
    fence: Fence,
    _busy: Busy,
}
struct Busy(Arc<Controller>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh
            .view
            .running = false;
    }
}
fn arguments(operation: &str, id: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "desktop".into(),
        "registration".into(),
        operation.into(),
        "--json".into(),
    ];
    if let Some(id) = id {
        args.extend(["--request-id".into(), id.into()]);
    }
    args
}
fn canonical_id(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok_and(|u| !u.is_nil() && u.to_string() == *id))
}
fn scope(target: &Value) -> Value {
    let mut value = target.clone();
    let object = value.as_object_mut().expect("checked target");
    object.remove("configuration");
    object.remove("original");
    object.insert("kind".into(), "original_instance_refresh".into());
    value
}
impl Controller {
    pub(in crate::desktop_service) fn refresh_view(&self) -> View {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut view = state.refresh.view.clone();
        view.configuration = state
            .refresh
            .original
            .as_ref()
            .map(|original| original.configuration.clone())
            .or_else(|| view.offer.as_ref().map(|offer| offer.configuration.clone()));
        view
    }
    fn refresh_failed(&self, code: &str) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.refresh.view.offer = None;
        state.refresh.view.code = code.into();
    }
    fn refresh_read(
        self: &Arc<Self>,
        route: Arc<Route>,
        args: Vec<String>,
        resources: Resources,
    ) -> Result<Value, &'static str> {
        let bytes = self
            .exchange_bytes(route, args, resources, Duration::from_secs(3), None)
            .0
            .map_err(|_| UNAVAILABLE)?;
        serde_json::from_slice(&bytes).map_err(|_| UNAVAILABLE)
    }
    fn refresh_info(
        self: &Arc<Self>,
        route: Arc<Route>,
        resources: Resources,
        configuration: &str,
        epoch: &str,
    ) -> Result<Value, &'static str> {
        let info = self.refresh_read(route, arguments("refresh-info", None), resources)?;
        let target = &info["target"];
        if info["schema"] != 1
            || info["refresh_abi"] != REFRESH_ABI
            || info["read_only"] != true
            || info["configuration"] != configuration
            || !target.is_object()
            || target["configuration"] != configuration
            || target["original"]["epoch"] != epoch
            || canonical_id(&target["original"]["epoch"]).is_none()
            || !target["original_generation"].is_u64()
            || !target["original"]["birth"].is_u64()
            || !target["original"]["pid"].is_u64()
            || target["original_image"] == target["replacement_image"]
        {
            return Err(CHANGED);
        }
        // The helper rechecks actual drain at Hold/Stop. A read cannot create
        // a drain permit, and busy originals are not stopped to make it true.
        if info["drained"] != true {
            return Err("refresh_draining");
        }
        Ok(target.clone())
    }
    fn refresh_lookup(self: &Arc<Self>, resources: Resources) -> Result<(), &'static str> {
        let (id, config, instance, expected_scope, route) = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let own = state.refresh.original.as_ref().ok_or(UNAVAILABLE)?;
            (
                own.id.clone(),
                own.configuration.clone(),
                own.instance.clone(),
                own.scope.clone(),
                own.route.clone(),
            )
        };
        let value = self.refresh_read(route, arguments("refresh-result", Some(&id)), resources)?;
        if value["schema"] != 1
            || value["refresh_abi"] != REFRESH_ABI
            || value["read_only"] != true
            || value["configuration"] != config
            || value["request_id"] != id
        {
            return Err(UNAVAILABLE);
        }
        let receipt = &value["receipt"];
        let settled = value["lost_original"].is_null()
            && value["outcome"] == "settled"
            && receipt["request"] == id
            && receipt["original"] == instance
            && receipt["scope"] == expected_scope
            && receipt["phase"] == "settled"
            && receipt["ready"]["instance"].is_object()
            && receipt["ready"]["generation"].is_u64();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.refresh.view.offer = None;
        state.refresh.view.settled = settled;
        state.refresh.view.code = if settled { "" } else { "refresh_unconfirmed" }.into();
        Ok(())
    }
    /// Read-only entry, also used after GUI restart. A durable original is
    /// only a lookup route, never a continuation or a fresh choice offer.
    pub(in crate::desktop_service) async fn observe_refresh(
        self: &Arc<Self>,
        ordinary: PathBuf,
        configuration: String,
        epoch: String,
    ) -> View {
        let Ok(charge) = self.gate.admit(Role::Fleet) else {
            return self.refresh_view();
        };
        let owner = self.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let result = (|| {
                if owner.stop.load(Ordering::Acquire) {
                    return Err(UNAVAILABLE);
                }
                let retained = {
                    let state = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some(original) = &state.refresh.original {
                        if original.configuration != configuration || original.epoch != epoch {
                            return Err(CHANGED);
                        }
                        Some(original.route.clone())
                    } else {
                        None
                    }
                };
                let selected = owner
                    .select_route(&ordinary)
                    .map_err(|refusal| refusal.logged())?;
                let route = retained.unwrap_or(Arc::new(selected));
                if !route.windows {
                    return Err("refresh_platform_unsupported");
                }
                let resources: Resources = Arc::new((charge, RoutePin::new(route.clone())?));
                let help = owner
                    .exchange_bytes(
                        route.clone(),
                        route.help_arguments(),
                        resources.clone(),
                        Duration::from_secs(3),
                        None,
                    )
                    .0
                    .map_err(|_| UNAVAILABLE)?;
                if !String::from_utf8_lossy(&help)
                    .lines()
                    .any(|l| l.trim() == REFRESH_MARKER)
                {
                    return Err("refresh_helper_unsupported");
                }
                if owner
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .refresh
                    .original
                    .is_some()
                {
                    return owner.refresh_lookup(resources);
                }
                let status = owner.refresh_read(
                    route.clone(),
                    arguments("status", None),
                    resources.clone(),
                )?;
                if status["schema"] != 1 || status["configuration"] != configuration {
                    return Err(CHANGED);
                }
                let pending = &status["pending_transition"];
                if pending.is_object() {
                    if !pending["refresh"].is_object() || pending["original"]["epoch"] != epoch {
                        return Err("refresh_review_required");
                    }
                    let id = canonical_id(&pending["request_id"])
                        .ok_or(UNAVAILABLE)?
                        .to_owned();
                    {
                        let mut state = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
                        state.refresh.original = Some(Original {
                            id: id.clone(),
                            configuration,
                            epoch,
                            instance: pending["original"].clone(),
                            scope: pending["refresh"].clone(),
                            route,
                        });
                        state.refresh.view.request_id = Some(id);
                    }
                    return owner.refresh_lookup(resources);
                }
                if !status["first_choice_lost"].is_null() {
                    return Err("refresh_review_required");
                }
                let target =
                    owner.refresh_info(route.clone(), resources, &configuration, &epoch)?;
                let mut state = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
                if state.refresh.view.offer.is_none()
                    || state.refresh.target.as_ref() != Some(&target)
                    || state
                        .refresh
                        .route
                        .as_ref()
                        .is_none_or(|r| r.cli != route.cli)
                {
                    state.refresh.view.offer = Some(Offer {
                        configuration,
                        epoch,
                        selection: uuid::Uuid::new_v4().to_string(),
                    });
                }
                state.refresh.target = Some(target);
                state.refresh.route = Some(route);
                state.refresh.view.code.clear();
                Ok(())
            })();
            if let Err(code) = result {
                owner.refresh_failed(code);
            }
        })
        .await;
        self.refresh_view()
    }
    pub(in crate::desktop_service) async fn confirm_refresh(
        self: &Arc<Self>,
        display: Offer,
        fence: Fence,
        confirmation: impl FnOnce(&Offer) -> bool + Send + 'static,
    ) -> Result<Option<Prepared>, &'static str> {
        let charge = self
            .gate
            .admit(Role::Fleet)
            .map_err(|_| "refresh_original_resources_pending")?;
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let (route, target) = {
                let mut state = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
                if state.refresh.view.offer.as_ref() != Some(&display)
                    || state.refresh.original.is_some()
                {
                    return Err(CHANGED);
                }
                state.refresh.view.running = true;
                (
                    state.refresh.route.clone().ok_or(UNAVAILABLE)?,
                    state.refresh.target.clone().ok_or(UNAVAILABLE)?,
                )
            };
            let busy = Busy(owner.clone());
            let resources: Resources = Arc::new((charge, RoutePin::new(route.clone())?));
            let check = || {
                fence()?;
                if owner.stop.load(Ordering::Acquire) {
                    return Err(CHANGED);
                }
                let fresh = owner.refresh_info(
                    route.clone(),
                    resources.clone(),
                    &display.configuration,
                    &display.epoch,
                )?;
                if fresh != target {
                    return Err(CHANGED);
                }
                fence()
            };
            check()?;
            if !confirmation(&display) {
                return Ok(None);
            }
            check()?;
            Ok(Some(Prepared {
                display,
                target,
                route,
                resources,
                fence,
                _busy: busy,
            }))
        })
        .await
        .map_err(|_| UNAVAILABLE)?
    }
    pub(in crate::desktop_service) async fn apply_refresh(
        self: &Arc<Self>,
        prepared: Prepared,
    ) -> Result<View, &'static str> {
        let owner = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let Prepared {
                display,
                target,
                route,
                resources,
                fence,
                _busy,
            } = prepared;
            fence()?;
            let fresh = owner.refresh_info(
                route.clone(),
                resources.clone(),
                &display.configuration,
                &display.epoch,
            )?;
            if fresh != target {
                return Err(CHANGED);
            }
            fence()?;
            let id = uuid::Uuid::new_v4().to_string();
            {
                let mut state = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
                state.refresh.original = Some(Original {
                    id: id.clone(),
                    configuration: display.configuration.clone(),
                    epoch: display.epoch.clone(),
                    instance: target["original"].clone(),
                    scope: scope(&target),
                    route: route.clone(),
                });
                state.refresh.view.offer = None;
                state.refresh.view.request_id = Some(id.clone());
                state.refresh.view.code = "refresh_unconfirmed".into();
            }
            let mut args = arguments("refresh", Some(&id));
            args.extend([
                "--scope".into(),
                serde_json::to_string(&target).map_err(|_| UNAVAILABLE)?,
            ]);
            let veto: Fence = {
                let owner = owner.clone();
                let route = route.clone();
                let resources = resources.clone();
                Arc::new(move || {
                    fence()?;
                    if owner.stop.load(Ordering::Acquire) {
                        return Err(CHANGED);
                    }
                    let fresh = owner.refresh_info(
                        route.clone(),
                        resources.clone(),
                        &display.configuration,
                        &display.epoch,
                    )?;
                    if fresh != target {
                        return Err(CHANGED);
                    }
                    fence()
                })
            };
            // Ignore success-shaped effect output: the exact original receipt
            // is the only result. Uncertain children retain the original charge.
            let _ = owner.exchange_bytes(
                route,
                args,
                resources.clone(),
                Duration::from_secs(30),
                Some(veto),
            );
            if let Err(code) = owner.refresh_lookup(resources) {
                owner.refresh_failed(code);
            }
            drop(_busy);
            Ok(owner.refresh_view())
        })
        .await
        .map_err(|_| "refresh_unconfirmed")?
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
