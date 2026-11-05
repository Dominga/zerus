use super::*;
use crate::desktop::notifications::windows::{
    tests::{post, session},
    Action, Command, Mappings, Native, NativeId, Outcome, Uncertainty,
};
use std::{cell::Cell, collections::VecDeque, rc::Rc, sync::Arc};

struct Fake {
    values: VecDeque<Result<bool, i32>>,
    steps: Vec<&'static str>,
    gate: Arc<AtomicBool>,
    bootstrap_fails: bool,
    cancel_after: Option<Rc<Cell<bool>>>,
    cancel_after_get: Option<Rc<Cell<bool>>>,
}
impl Fake {
    fn new(values: impl IntoIterator<Item = Result<bool, i32>>) -> Self {
        Self {
            values: values.into_iter().collect(),
            steps: Vec::new(),
            gate: Arc::new(AtomicBool::new(false)),
            bootstrap_fails: false,
            cancel_after: None,
            cancel_after_get: None,
        }
    }
    fn run(&mut self) -> Outcome {
        let scope = session();
        let mut mappings = Mappings::new(scope.clone()).unwrap();
        assert!(mappings.finish_restoration());
        mappings.execute(&post(&scope, 1), self, || None)
    }
}
impl Platform for Fake {
    fn setting(&mut self) -> Result<bool, i32> {
        self.steps.push("getter");
        if let Some(cancel) = &self.cancel_after_get {
            cancel.set(true);
        }
        self.values
            .pop_front()
            .expect("no additional getter or retry")
    }
    fn bootstrap(&mut self, cancelled: &dyn Fn() -> bool) -> Result<(), Refusal> {
        if cancelled() {
            return Err(Refusal::Cancelled);
        }
        self.steps.extend(["bootstrap", "remove"]);
        if let Some(cancel) = &self.cancel_after {
            cancel.set(true);
        }
        if self.bootstrap_fails {
            Err(Refusal::GetterFailed)
        } else {
            Ok(())
        }
    }
}
impl Native for Fake {
    fn ensure_post_ready(&mut self, cancelled: &dyn Fn() -> bool) -> Result<(), Refusal> {
        let gate = self.gate.clone();
        ensure_enabled(self, &gate, cancelled)
    }
    fn prepare(&mut self, _: &Command, _: &NativeId) -> Result<(), Refusal> {
        self.steps.push("prepare");
        Ok(())
    }
    fn post(&mut self, _: &Command, id: &NativeId) -> Result<NativeId, ()> {
        self.steps.push("post");
        Ok(id.clone())
    }
    fn withdraw(&mut self, _: &NativeId) -> Result<(), ()> {
        self.steps.push("withdraw");
        Ok(())
    }
    fn close(&mut self) -> bool {
        true
    }
}

#[test]
fn cold_identity_bootstraps_then_rechecks_before_the_original_post() {
    let mut fake = Fake::new([Err(NOT_FOUND), Ok(true)]);
    assert!(matches!(fake.run(), Outcome::Delivered { .. }));
    assert_eq!(
        fake.steps,
        ["getter", "bootstrap", "remove", "getter", "prepare", "post"]
    );
}
#[test]
fn real_disabled_setting_refuses_without_bootstrap_or_post() {
    let mut fake = Fake::new([Ok(false)]);
    assert_eq!(
        fake.run(),
        Outcome::Refused {
            code: Refusal::PermissionDenied
        }
    );
    assert_eq!(fake.steps, ["getter"]);
    assert!(!fake.gate.load(Ordering::Acquire));
}
#[test]
fn other_getter_errors_refuse_with_the_real_closed_code() {
    let mut fake = Fake::new([Err(-1)]);
    let result = fake.run();
    assert_eq!(
        result,
        Outcome::Refused {
            code: Refusal::GetterFailed
        }
    );
    assert_eq!(
        serde_json::to_value(&result).unwrap()["code"],
        "getter_failed"
    );
    assert_eq!(fake.steps, ["getter"]);
    assert!(!fake.gate.load(Ordering::Acquire));
}
#[test]
fn a_second_error_never_loops_and_a_new_worker_cannot_retry_bootstrap() {
    let mut first = Fake::new([Err(NOT_FOUND), Err(NOT_FOUND)]);
    assert_eq!(
        first.run(),
        Outcome::Refused {
            code: Refusal::GetterFailed
        }
    );
    assert_eq!(first.steps, ["getter", "bootstrap", "remove", "getter"]);
    let mut replacement = Fake::new([Err(NOT_FOUND)]);
    replacement.gate = first.gate.clone();
    assert_eq!(
        replacement.run(),
        Outcome::Refused {
            code: Refusal::GetterFailed
        }
    );
    assert_eq!(replacement.steps, ["getter"]);
}
#[test]
fn disabled_on_recheck_and_bootstrap_failure_never_post() {
    let mut disabled = Fake::new([Err(NOT_FOUND), Ok(false)]);
    assert_eq!(
        disabled.run(),
        Outcome::Refused {
            code: Refusal::PermissionDenied
        }
    );
    assert_eq!(disabled.steps, ["getter", "bootstrap", "remove", "getter"]);
    let mut failed = Fake::new([Err(NOT_FOUND)]);
    failed.bootstrap_fails = true;
    assert_eq!(
        failed.run(),
        Outcome::Refused {
            code: Refusal::GetterFailed
        }
    );
    assert_eq!(failed.steps, ["getter", "bootstrap", "remove"]);
    assert!(failed.gate.load(Ordering::Acquire));
}
#[test]
fn a_warm_identity_never_needs_bootstrap() {
    let mut fake = Fake::new([Ok(true)]);
    assert!(matches!(fake.run(), Outcome::Delivered { .. }));
    assert_eq!(fake.steps, ["getter", "prepare", "post"]);
    assert!(!fake.gate.load(Ordering::Acquire));
}
#[test]
fn unrestored_or_cancelled_work_never_bootstraps() {
    let scope = session();
    let mut fake = Fake::new([]);
    let mut mappings = Mappings::new(scope.clone()).unwrap();
    assert_eq!(
        mappings.execute(&post(&scope, 1), &mut fake, || None),
        Outcome::Refused {
            code: Refusal::NotReady
        }
    );
    assert!(fake.steps.is_empty());
    assert!(mappings.finish_restoration());
    assert_eq!(
        mappings.execute(&post(&scope, 2), &mut fake, || Some(Uncertainty::HostLost)),
        Outcome::Refused {
            code: Refusal::Cancelled
        }
    );
    assert!(fake.steps.is_empty());
    assert!(!fake.gate.load(Ordering::Acquire));
}
#[test]
fn cancellation_during_bootstrap_keeps_the_original_post_unshown() {
    let cancel = Rc::new(Cell::new(false));
    let mut fake = Fake::new([Err(NOT_FOUND)]);
    fake.cancel_after = Some(cancel.clone());
    let scope = session();
    let mut mappings = Mappings::new(scope.clone()).unwrap();
    assert!(mappings.finish_restoration());
    assert_eq!(
        mappings.execute(&post(&scope, 1), &mut fake, || cancel
            .get()
            .then_some(Uncertainty::HostLost)),
        Outcome::Refused {
            code: Refusal::Cancelled
        }
    );
    assert_eq!(fake.steps, ["getter", "bootstrap", "remove"]);
}

#[test]
fn cancellation_after_the_getter_prevents_even_the_bootstrap() {
    let cancel = Rc::new(Cell::new(false));
    let mut fake = Fake::new([Err(NOT_FOUND)]);
    fake.cancel_after_get = Some(cancel.clone());
    let scope = session();
    let mut mappings = Mappings::new(scope.clone()).unwrap();
    assert!(mappings.finish_restoration());
    assert_eq!(
        mappings.execute(&post(&scope, 1), &mut fake, || cancel
            .get()
            .then_some(Uncertainty::HostLost)),
        Outcome::Refused {
            code: Refusal::Cancelled
        }
    );
    assert_eq!(fake.steps, ["getter"]);
    assert!(!fake.gate.load(Ordering::Acquire));
}
#[test]
fn withdrawing_an_original_toast_never_queries_or_bootstraps_identity() {
    let scope = session();
    let mut fake = Fake::new([Ok(true), Err(NOT_FOUND)]);
    let mut mappings = Mappings::new(scope.clone()).unwrap();
    assert!(mappings.finish_restoration());
    let original = post(&scope, 1);
    let Outcome::Delivered { native_id } = mappings.execute(&original, &mut fake, || None) else {
        panic!("first post");
    };
    let mut binding = original.binding.clone();
    binding.seq = 2;
    binding.request_id = uuid::Uuid::new_v4().to_string();
    let withdrawal = Command {
        binding,
        action: Action::Withdraw {
            token: original.action.token().into(),
            native_id,
        },
    };
    assert!(matches!(
        mappings.execute(&withdrawal, &mut fake, || None),
        Outcome::Delivered { .. }
    ));
    assert_eq!(
        fake.steps,
        ["getter", "prepare", "post", "prepare", "withdraw"]
    );
    assert_eq!(fake.values.len(), 1);
    assert!(!fake.gate.load(Ordering::Acquire));
}
