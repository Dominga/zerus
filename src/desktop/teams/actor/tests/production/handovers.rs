//! A hand-over asks about ownership, not the stage that the task command owns.
use super::*;
struct Handover {
    native: native::Native,
    calls: usize,
    refused: bool,
}
impl ports::Ports for Handover {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> ports::Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: Some(Default::default()),
        })
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> ports::Result<()> {
        Ok(())
    }
    fn free(&mut self, t: &Team, m: &Member) -> ports::Result<bool> {
        ports::Ports::free(&mut self.native, t, m)
    }
    fn deliver(&mut self, t: &Team, n: &Node, m: &Member, id: &str) -> ports::DeliveryResult {
        self.calls += 1;
        let reply = if self.refused {
            serde_json::json!({"schema":1,"id":id,"state":"refused","error":"The task has no delivered execution to hand over"})
        } else {
            serde_json::json!({"schema":1,"id":id,"state":"submitted","error":""})
        };
        crate::desktop::teams::child::FIXTURE_EXIT
            .with(|s| *s.borrow_mut() = Some(reply.to_string().into_bytes()));
        let result = ports::Ports::deliver(&mut self.native, t, n, m, id);
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| s.borrow_mut().take());
        result
    }
}
fn prepared(mode: &str) -> (Harness, u32, Actor) {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    if mode == "empty" {
        h.intent(node, "m1");
        h.observe(node, ops::TaskStage::InProgress);
    } else {
        h.submit(node, "m1");
    }
    if matches!(mode, "review" | "done" | "return done") {
        h.report(node, "m1");
    }
    if mode.contains("done") {
        h.observe(node, ops::TaskStage::Done);
    }
    let op = if mode == "return done" {
        Operation::Return {
            node,
            reason: "Revise the closed result".into(),
        }
    } else {
        Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Hand the current task over".into(),
        }
    };
    h.run(&lead, op).unwrap();
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    assert!(
        h.store.own_task_current(TEAM, node, generation).unwrap(),
        "{mode}"
    );
    (h, node, lead)
}
#[test]
fn reassign_and_return_use_the_owned_execution_at_every_supported_task_stage() {
    for mode in ["running", "review", "done", "return done", "empty"] {
        let (mut h, node, lead) = prepared(mode);
        let native = native(&h);
        let machine = native.machine.clone();
        let mut p = Handover {
            native,
            calls: 0,
            refused: mode == "empty",
        };
        let mut runtime = Runtime::open(h.temp.path(), &machine).unwrap();
        for pass in 0..3 {
            runtime
                .tick(&mut h.store, &mut p, TEAM, 100 + pass * 60_000, false)
                .unwrap();
        }
        assert_eq!(p.calls, 1, "{mode}");
        let effect = h
            .store
            .effects(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|e| e.kind == "reopen")
            .unwrap();
        assert_eq!(
            effect.state,
            if mode == "empty" {
                "refused"
            } else {
                "submitted"
            },
            "{mode}"
        );

        let decision = h
            .store
            .decisions(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|d| matches!(d.kind.as_str(), "return" | "reassign"))
            .unwrap();
        assert_eq!(
            decision.state,
            crate::teams::store::DecisionState::Settled,
            "{mode}"
        );
        h.run(
            &lead,
            Operation::Cancel {
                node,
                reason: "A further decision is possible".into(),
            },
        )
        .unwrap();
    }
}
