//! Capture the bytes built at each production adapter's call site. The stopped
//! fixture never starts a child; the matrix then executes those exact bytes.
use super::super::super::{native::Native, ports::Ports};
use super::*;
use std::sync::{atomic::AtomicBool, Arc};
impl Fixture {
    fn vector(&self, call: impl FnOnce(&mut Native)) -> (Vec<String>, Value) {
        use super::super::super::native::{CAPTURED_CHILD, TEST_INVENTORY};
        struct Reset(Option<Value>);
        impl Drop for Reset {
            fn drop(&mut self) {
                TEST_INVENTORY.with(|s| *s.borrow_mut() = self.0.take());
            }
        }
        let team = self.h.store.team(TEAM).unwrap().unwrap();
        let _reset = Reset(TEST_INVENTORY.with(|s| {
            s.replace(Some(
                json!({"node_id":team.home,"organization":{"projects":[]}}),
            ))
        }));
        CAPTURED_CHILD.with(|s| *s.borrow_mut() = None);
        let mut native = Native {
            config: self.config.clone(),
            machine: team.home,
            epoch: "command-fixture".into(),
            stop: Arc::new(AtomicBool::new(true)),
        };
        call(&mut native);
        CAPTURED_CHILD
            .with(|s| s.borrow_mut().take())
            .expect("production adapter built no child")
    }
    pub(super) fn delivery_vector(&self, id: &str, member: &str) -> (Vec<String>, Value) {
        let team = self.h.store.team(TEAM).unwrap().unwrap();
        let node = self.h.node(self.node);
        let member = self.h.store.member(TEAM, member).unwrap().unwrap();
        self.vector(|p| {
            p.deliver(&team, &node, &member, id);
        })
    }
    pub(super) fn finish_vector(&self) -> (Vec<String>, Value) {
        let team = self.h.store.team(TEAM).unwrap().unwrap();
        let node = self.h.node(self.node);
        let decision = self
            .h
            .store
            .pending_accepts(TEAM, 16, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|d| d.node == node.id)
            .unwrap();
        self.vector(|p| {
            let _ = p.finish(&team, &node, &decision, &uuid::Uuid::new_v4().to_string());
        })
    }
    pub(super) fn stage_vector(
        &self,
        effect: &crate::teams::store::StageTaskEffect,
    ) -> (Vec<String>, Value) {
        let team = self.h.store.team(TEAM).unwrap().unwrap();
        self.vector(|p| {
            let _ = p.stage(&team, effect, &uuid::Uuid::new_v4().to_string());
        })
    }
    pub(super) fn clear_vector(&self, node: u32, task: &TaskRef) -> (Vec<String>, Value) {
        let team = self.h.store.team(TEAM).unwrap().unwrap();
        let keyed = crate::teams::store::timers::KeyedTask {
            node,
            task: task.clone(),
        };
        self.vector(|p| {
            let _ = p.clear_stage(&team, &keyed, &uuid::Uuid::new_v4().to_string());
        })
    }
    pub(super) fn cancel_vector(&self) -> (Vec<String>, Value) {
        let team = self.h.store.team(TEAM).unwrap().unwrap();
        let pending = Native::pending_cancels(&self.h.store, TEAM, self.node - 1)
            .unwrap()
            .into_iter()
            .find(|p| p.node == self.node)
            .unwrap();
        self.vector(|p| {
            let _ = p.cancel(&team, &pending, &uuid::Uuid::new_v4().to_string());
        })
    }
}
