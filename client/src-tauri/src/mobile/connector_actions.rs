//! One-use connector lifecycle intents on the existing Mobile effects lane.
use super::*;
use mobile::{
    connector_actions::{self as core, Kind},
    enrollment::Target,
    Failure,
};
use serde::Serialize;
use std::{collections::VecDeque, sync::atomic::AtomicBool};
struct Plan {
    target: Target,
    kind: Kind,
}
#[derive(Default)]
pub(super) struct Plans(VecDeque<(String, Plan)>);
#[derive(Serialize)]
pub struct Form {
    pub(super) handle: String,
    target: Target,
    kind: Kind,
}
#[derive(Serialize)]
pub struct Reply {
    notice: &'static str,
}
pub(super) struct Active<'a>(pub(super) &'a AtomicBool);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl Inner {
    pub(super) fn connector_open(&self, requested: &Target, kind: Kind) -> Result<Form, String> {
        if self.blocked {
            return Err(REPLAY.into());
        }
        let view = self.page().view();
        if view.workspace_busy || view.workspace_error.is_some() {
            return Err(core::CHANGED.into());
        }
        let target = core::admit(view.workspace.as_ref().ok_or(core::CHANGED)?, kind)?;
        if target != *requested {
            return Err(core::CHANGED.into());
        }
        let handle = uuid::Uuid::new_v4().to_string();
        let mut plans = self
            .connector_plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while plans.0.len() >= 8 {
            plans.0.pop_front();
        }
        plans.0.push_back((
            handle.clone(),
            Plan {
                target: target.clone(),
                kind,
            },
        ));
        Ok(Form {
            handle,
            target,
            kind,
        })
    }
    fn connector_take(&self, handle: &str) -> Result<(Plan, Active<'_>), String> {
        if self.blocked {
            return Err(REPLAY.into());
        }
        let mut plans = self
            .connector_plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let index = plans
            .0
            .iter()
            .position(|(id, _)| id == handle)
            .ok_or(core::CHANGED)?;
        if self.effects.swap(true, Ordering::AcqRel) {
            return Err("A mobile action is already running.".into());
        }
        let active = Active(&self.effects);
        Ok((plans.0.remove(index).ok_or(core::CHANGED)?.1, active))
    }
    fn connector_close(&self, handle: &str) {
        self.connector_plans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .0
            .retain(|(id, _)| id != handle);
    }
    /// The button's one-use plan, admitted only with the status lane free: a Stop
    /// can never be followed by a status reply that began before it.
    pub(super) fn connector_apply(
        self: &Arc<Self>,
        handle: &str,
        apply: impl FnOnce(&Target, Kind) -> Result<core::Answer, Failure>,
    ) -> Result<&'static str, String> {
        let (plan, _active) = self.connector_take(handle)?;
        if !self.page().read_workspace() {
            return Err(core::CHANGED.into());
        }
        self.connector_run(&plan.target, plan.kind, apply)
    }
    /// One lifecycle call on the taken lane. Stop disarms at its admission, saved
    /// before it is sent: a crash here must not bring the connector back at the
    /// next start. A successful Start is what arms it again, and a definite refusal
    /// restores the choice, because nothing happened.
    pub(super) fn connector_run(
        self: &Arc<Self>,
        target: &Target,
        kind: Kind,
        apply: impl FnOnce(&Target, Kind) -> Result<core::Answer, Failure>,
    ) -> Result<&'static str, String> {
        if kind == Kind::Stop {
            self.page()
                .autostart_stop_chosen(&target.node_id, target.generation.as_deref());
            self.save_autostart();
        }
        (self.notify)();
        let answer = match apply(target, kind) {
            Ok(answer) => {
                let notice = answer.notice;
                if kind == Kind::Start {
                    self.page().autostart_person_started(&answer.workspace);
                }
                if self.answered(Ok(answer.workspace)) {
                    self.phones();
                }
                Ok(notice)
            }
            // Only the connector's closed result for this very call: nothing happened.
            Err(Failure::StopRefused) if kind == Kind::Stop => {
                self.page()
                    .autostart_stop_refused(&target.node_id, target.generation.as_deref());
                self.save_autostart();
                self.page()
                    .workspace_answered(Err(mobile::STOP_REFUSED.into()));
                Err(mobile::STOP_REFUSED.into())
            }
            Err(failure) => {
                self.page()
                    .workspace_answered(Err(mobile::WORKSPACE_UNCONFIRMED.into()));
                Err(failure.message())
            }
        };
        (self.notify)();
        answer
    }
}
#[tauri::command(async)]
pub fn mobile_connector_open(
    mobile: State<'_, Mobile>,
    target: Target,
    kind: Kind,
) -> Result<Form, String> {
    mobile.0.connector_open(&target, kind)
}
#[tauri::command(async)]
pub fn mobile_connector_close(mobile: State<'_, Mobile>, handle: String) {
    mobile.0.connector_close(&handle);
}
#[tauri::command(async)]
pub fn mobile_connector_apply(mobile: State<'_, Mobile>, handle: String) -> Result<Reply, String> {
    let notice = mobile.0.connector_apply(&handle, |target, kind| {
        let (connector, config) = workspace_runtime();
        core::apply(&connector, &config, target, kind)
    })?;
    Ok(Reply { notice })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Arc<Inner>, Target) {
        let inner = Arc::new(Inner::new(
            false,
            super::super::tests::service_calls_for_plans(),
            Box::new(|| {}),
        ));
        let mut w=mobile::workspace(serde_json::json!({"state":"active","encrypted":true,"config_revision":"a".repeat(64),"server_url":"https://relay.example.invalid","node_id":"node-old","workspace_id":"ws-old","running":true,"connector_state":"running","generation":"e".repeat(32)}).as_object().unwrap());
        w.stop_supported = true;
        let target = core::admit(&w, Kind::Stop).unwrap();
        inner.page().workspace_answered(Ok(w));
        (inner, target)
    }
    #[test]
    fn plans_are_bounded_and_consumed_before_work() {
        let (i, t) = fixture();
        let first = i.connector_open(&t, Kind::Stop).unwrap();
        for _ in 0..8 {
            i.connector_open(&t, Kind::Stop).unwrap();
        }
        assert!(i.connector_take(&first.handle).is_err());
        let next = i.connector_open(&t, Kind::Stop).unwrap();
        let (_, active) = i.connector_take(&next.handle).unwrap();
        drop(active);
        assert!(i.connector_take(&next.handle).is_err());
    }
    #[test]
    fn admission_never_retargets_changed_metadata() {
        let (i, mut t) = fixture();
        t.generation = Some("b".repeat(32));
        assert!(i.connector_open(&t, Kind::Stop).is_err());
        assert!(i.connector_open(&t, Kind::Start).is_err());
    }
    #[test]
    fn old_close_keeps_the_new_plan() {
        let (i, t) = fixture();
        let first = i.connector_open(&t, Kind::Stop).unwrap();
        let next = i.connector_open(&t, Kind::Stop).unwrap();
        i.connector_close(&first.handle);
        assert!(i.connector_take(&next.handle).is_ok());
    }
}
