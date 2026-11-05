//! Team actions (task #7) against the fake `hgs` (tests/support/fake_hgs.py),
//! never the real CLI: each action's request, its receipt, the owner's
//! confirmation, refusals and unknown outcomes that are never sent again.
mod common;

use common::FakeHgs;
use serde_json::{json, Value};
use std::time::Duration;
use zerus_client_core::teams::action::{classify, prepare, run, TeamAction, TeamCall, TeamOutcome};
use zerus_client_core::Exit;

const TEAM: &str = "33fbafeb-8291-4eda-a00c-6e1448ec6f20";
const NEW_TEAM: &str = "6d0aabee-1111-4222-8333-444455556666";

fn fake(test: &str) -> FakeHgs {
    FakeHgs::new(test, json!({"": {}, "studio": {}}))
}

fn reply(fake: &FakeHgs, op: &str, host: &str, answer: Value) {
    let name = format!(
        "team-{op}-{}.json",
        if host.is_empty() { "local" } else { host }
    );
    std::fs::write(fake.dir.join(name), answer.to_string()).unwrap();
}

/// The team requests the fake received, in order.
fn requests(fake: &FakeHgs) -> Vec<(String, Value)> {
    fake.log()
        .into_iter()
        .filter(|entry| entry.get("team").is_some())
        .map(|entry| {
            (
                entry["team"].as_str().unwrap().to_owned(),
                entry["request"].clone(),
            )
        })
        .collect()
}

fn call(action: TeamAction) -> TeamCall {
    TeamCall {
        computer: String::new(),
        team: TEAM.into(),
        team_name: "Zerus Next for the fork".into(),
        action,
        confirmed: true,
        request_id: None,
    }
}

/// Every action the Qt Teams UI offers: its operation and the fields it sends.
fn actions() -> Vec<(TeamAction, &'static str, Value)> {
    vec![
        (
            TeamAction::Approve {
                phase: None,
                plan_mode: Some("free".into()),
                confirm_widening: true,
                alongside: false,
            },
            "approve",
            json!({"plan_mode": "free", "confirm_widening": true}),
        ),
        (
            TeamAction::Approve {
                phase: Some(2),
                plan_mode: None,
                confirm_widening: false,
                alongside: true,
            },
            "approve-phase",
            json!({"phase": 2, "alongside": true}),
        ),
        (
            TeamAction::Accept {
                task: 6,
                reason: String::new(),
                manual: false,
            },
            "accept",
            json!({"task": 6}),
        ),
        (
            TeamAction::Accept {
                task: 6,
                reason: "Done by hand".into(),
                manual: true,
            },
            "accept",
            json!({"task": 6, "reason": "Done by hand", "manual": true}),
        ),
        (
            TeamAction::Reject {
                task: 6,
                reason: "Add the paging test".into(),
                hold: true,
                fresh: false,
            },
            "reject",
            json!({"task": 6, "reason": "Add the paging test", "hold": true}),
        ),
        (
            TeamAction::Reply {
                question: "q1".into(),
                text: "Use the fixtures".into(),
            },
            "reply",
            json!({"question": "q1", "text": "Use the fixtures"}),
        ),
        (
            TeamAction::Reassign {
                task: 6,
                member: "ui1".into(),
                subteam: None,
            },
            "reassign",
            json!({"task": 6, "member": "ui1"}),
        ),
        (
            TeamAction::TakeOver { task: 6 },
            "take-over",
            json!({"task": 6}),
        ),
        (
            TeamAction::Cancel {
                task: 6,
                subteam: Some("stop".into()),
            },
            "cancel",
            json!({"task": 6, "subteam": "stop"}),
        ),
        (TeamAction::Pause, "pause", json!({})),
        (TeamAction::Resume, "resume", json!({})),
        (TeamAction::Stop, "stop", json!({})),
        (TeamAction::Archive, "archive", json!({})),
        (
            TeamAction::DecideProposal {
                proposal: "p4".into(),
                accept: true,
                expected_hash: "a".repeat(64),
                reason: String::new(),
                answer: String::new(),
            },
            "decide-proposal",
            json!({"proposal": "p4", "accept": true, "expected_hash": "a".repeat(64)}),
        ),
        (
            TeamAction::UseLastReply {
                task: 6,
                attempt: "a1".into(),
            },
            "use-last-reply",
            json!({"task": 6, "attempt": "a1"}),
        ),
        (
            TeamAction::DraftUpdate {
                revision: 3,
                set: json!({"name": "Checkout v2"}),
            },
            "draft",
            json!({"action": "update", "revision": 3, "set": {"name": "Checkout v2"}}),
        ),
        (
            TeamAction::DraftSeen {
                fields: vec!["members".into()],
            },
            "draft",
            json!({"action": "seen", "fields": ["members"]}),
        ),
        (
            TeamAction::DraftCreate {
                approve: false,
                confirm_widening: false,
            },
            "draft",
            json!({"action": "create"}),
        ),
        (
            TeamAction::DraftDiscard {
                stop_lead: true,
                reason: String::new(),
            },
            "draft",
            json!({"action": "discard", "stop_lead": true}),
        ),
        (
            TeamAction::Settings {
                settings: json!({"branch_per_task": true, "land": false}),
            },
            "settings",
            json!({"settings": {"branch_per_task": true, "land": false}}),
        ),
    ]
}

#[test]
fn every_action_sends_its_request_once_and_gives_the_receipt() {
    let fake = fake("team_actions_every");
    let mut sent = 0;
    for (action, op, fields) in actions() {
        reply(
            &fake,
            op,
            "",
            json!({"stdout": {"status": "ok", "proposal": "p4"}, "echo": true}),
        );
        let outcome = run(&fake.path(), &call(action.clone()));
        let TeamOutcome::Done {
            receipt,
            request_id,
        } = &outcome
        else {
            panic!("{op}: {outcome:?}")
        };
        assert_eq!(
            receipt["request_id"],
            json!(request_id),
            "{op}: the receipt of this request"
        );
        sent += 1;
        let log = requests(&fake);
        assert_eq!(log.len(), sent, "{op} ran once");
        let (logged_op, request) = log.last().unwrap();
        assert_eq!(logged_op, op);
        let mut expected = fields.as_object().unwrap().clone();
        expected.insert("team".into(), json!(TEAM));
        expected.insert("by".into(), json!("human"));
        expected.insert("request_id".into(), json!(request_id));
        assert_eq!(request, &Value::Object(expected), "{op}");
    }
    // A draft start names no team; its receipt is the new draft's id.
    let start = TeamAction::DraftStart {
        folder: "C:\\work\\example".into(),
        name: "Checkout".into(),
        lead: json!({"role": "Lead", "agent": "claude", "session": {"host": "", "name": "claude/example/lead"}}),
        project: "Example".into(),
        project_id: String::new(),
        template: "feature".into(),
    };
    reply(
        &fake,
        "draft",
        "",
        json!({"stdout": {"team": NEW_TEAM, "revision": 1}}),
    );
    let outcome = run(
        &fake.path(),
        &TeamCall {
            team: String::new(),
            ..call(start)
        },
    );
    assert!(
        matches!(&outcome, TeamOutcome::Done { receipt, .. } if receipt["team"] == NEW_TEAM),
        "{outcome:?}"
    );
    let (_, request) = requests(&fake).pop().unwrap();
    assert_eq!(request["action"], "start");
    assert!(request.get("team").is_none());
    assert_eq!(
        (request["folder"].as_str(), request["template"].as_str()),
        (Some("C:\\work\\example"), Some("feature"))
    );
    assert_eq!(request["lead"]["session"]["name"], "claude/example/lead");
}

#[test]
fn destructive_actions_and_approvals_ask_first_and_send_nothing() {
    let fake = fake("team_actions_confirm");
    for action in [
        TeamAction::Stop,
        TeamAction::Archive,
        TeamAction::Cancel {
            task: 6,
            subteam: None,
        },
        TeamAction::TakeOver { task: 6 },
        TeamAction::DraftDiscard {
            stop_lead: false,
            reason: String::new(),
        },
        TeamAction::Approve {
            phase: None,
            plan_mode: None,
            confirm_widening: false,
            alongside: false,
        },
        TeamAction::Approve {
            phase: Some(2),
            plan_mode: None,
            confirm_widening: false,
            alongside: false,
        },
    ] {
        reply(&fake, action.op(), "", json!({"stdout": {"status": "ok"}}));
        let outcome = run(
            &fake.path(),
            &TeamCall {
                confirmed: false,
                ..call(action.clone())
            },
        );
        let TeamOutcome::Confirm { confirmation } = &outcome else {
            panic!("{action:?}: {outcome:?}")
        };
        assert!(
            !confirmation.title.is_empty() && !confirmation.action.is_empty(),
            "{confirmation:?}"
        );
        if matches!(action, TeamAction::Stop | TeamAction::Archive) {
            assert!(
                confirmation.text.contains("Zerus Next for the fork"),
                "{}",
                confirmation.text
            );
        }
    }
    assert!(
        requests(&fake).is_empty(),
        "nothing ran before the owner confirmed"
    );
    // Ordinary actions need no confirmation.
    reply(&fake, "pause", "", json!({"stdout": {"status": "paused"}}));
    let outcome = run(
        &fake.path(),
        &TeamCall {
            confirmed: false,
            ..call(TeamAction::Pause)
        },
    );
    assert!(matches!(outcome, TeamOutcome::Done { .. }), "{outcome:?}");
}

#[test]
fn a_refusal_fails_and_an_unknown_outcome_is_uncertain_and_never_sent_again() {
    let fake = fake("team_actions_outcomes");
    let accept = || {
        call(TeamAction::Accept {
            task: 6,
            reason: String::new(),
            manual: false,
        })
    };
    // hgs refused it: nothing was applied, its words show.
    reply(
        &fake,
        "accept",
        "",
        json!({"exit": 1, "stderr": "hgs: Task #6 has no report to decide\n"}),
    );
    let outcome = run(&fake.path(), &accept());
    assert!(
        matches!(&outcome, TeamOutcome::Failed { detail, .. } if detail == "hgs: Task #6 has no report to decide"),
        "{outcome:?}"
    );
    // No valid receipt: it may have been applied; it ran once.
    reply(&fake, "accept", "", json!({"stdout": "not a receipt"}));
    let before = requests(&fake).len();
    let outcome = run(&fake.path(), &accept());
    let TeamOutcome::Uncertain { detail, request_id } = &outcome else {
        panic!("{outcome:?}")
    };
    assert!(detail.contains("Read the team"), "{detail}");
    assert_eq!(
        requests(&fake).len(),
        before + 1,
        "an unknown outcome is never sent again by itself"
    );
    // The owner sends it again on purpose: the same request identity, which
    // the store answers with its receipt.
    reply(
        &fake,
        "accept",
        "",
        json!({"stdout": {"status": "accepted", "task": 6}, "echo": true}),
    );
    let again = run(
        &fake.path(),
        &TeamCall {
            request_id: Some(request_id.clone()),
            ..accept()
        },
    );
    assert!(
        matches!(&again, TeamOutcome::Done { request_id: id, .. } if id == request_id),
        "{again:?}"
    );
    let log = requests(&fake);
    assert_eq!(
        log[log.len() - 2].1["request_id"],
        log[log.len() - 1].1["request_id"]
    );
    // A call that outlives its time is uncertain too, and ran once.
    reply(
        &fake,
        "accept",
        "",
        json!({"sleep": 5, "stdout": {"status": "accepted"}}),
    );
    let (mut invocation, request_id) = prepare(&accept()).unwrap();
    invocation.timeout = Duration::from_millis(1500);
    let before = requests(&fake).len();
    let outcome = classify(
        &accept().action,
        &request_id,
        zerus_client_core::run(&fake.path(), &invocation),
    );
    assert!(
        matches!(outcome, TeamOutcome::Uncertain { .. }),
        "{outcome:?}"
    );
    assert_eq!(requests(&fake).len(), before + 1);
    // A receipt for another proposal does not prove this decision.
    reply(
        &fake,
        "decide-proposal",
        "",
        json!({"stdout": {"status": "accepted", "proposal": "p9"}}),
    );
    let decide = TeamAction::DecideProposal {
        proposal: "p4".into(),
        accept: true,
        expected_hash: "b".repeat(64),
        reason: String::new(),
        answer: String::new(),
    };
    assert!(matches!(
        run(&fake.path(), &call(decide)),
        TeamOutcome::Uncertain { .. }
    ));
    // A draft start without the new draft's id is uncertain as well.
    reply(&fake, "draft", "", json!({"stdout": {"status": "ok"}}));
    let start = TeamAction::DraftStart {
        folder: "C:\\work\\example".into(),
        name: String::new(),
        lead: json!({"agent": "claude"}),
        project: String::new(),
        project_id: String::new(),
        template: String::new(),
    };
    assert!(matches!(
        run(
            &fake.path(),
            &TeamCall {
                team: String::new(),
                ..call(start)
            }
        ),
        TeamOutcome::Uncertain { .. }
    ));
    // An owner-only settings change refused by hgs is failed, with hgs's
    // own words, and is never resent.
    reply(
        &fake,
        "settings",
        "",
        json!({"exit": 1, "stderr": "hgs: only the owner can change team settings\n"}),
    );
    let settings = TeamAction::Settings {
        settings: json!({"land": true}),
    };
    let before = requests(&fake).len();
    let outcome = run(&fake.path(), &call(settings.clone()));
    assert!(
        matches!(&outcome, TeamOutcome::Failed { detail, .. } if detail == "hgs: only the owner can change team settings"),
        "{outcome:?}"
    );
    assert_eq!(requests(&fake).len(), before + 1, "a refusal is not resent");
}

#[test]
fn exits_that_never_reached_the_store_fail_and_the_others_are_uncertain() {
    let action = TeamAction::Pause;
    let id = "00000000-0000-4000-8000-000000000001";
    assert!(matches!(
        classify(&action, id, Exit::StartFailed("not found".into())),
        TeamOutcome::Failed { .. }
    ));
    for exit in [
        Exit::TimedOut,
        Exit::WriteFailed,
        Exit::Abnormal {
            stderr: String::new(),
        },
    ] {
        let outcome = classify(&action, id, exit);
        assert!(
            matches!(&outcome, TeamOutcome::Uncertain { request_id, .. } if request_id == id),
            "{outcome:?}"
        );
    }
    let failed = classify(
        &action,
        id,
        Exit::Code {
            code: 3,
            stdout: Vec::new(),
            stderr: " \n".into(),
        },
    );
    assert!(
        matches!(failed, TeamOutcome::Failed { detail, .. } if detail == "hgs exited with code 3")
    );
}

#[test]
fn requests_that_cannot_be_made_never_run_and_a_peer_gets_its_alias() {
    let fake = fake("team_actions_refused");
    reply(
        &fake,
        "reject",
        "",
        json!({"stdout": {"status": "rejected"}}),
    );
    reply(&fake, "pause", "", json!({"stdout": {"status": "paused"}}));
    let refused = |call: TeamCall| matches!(run(&fake.path(), &call), TeamOutcome::Failed { .. });
    assert!(refused(TeamCall {
        team: "--all".into(),
        ..call(TeamAction::Pause)
    }));
    assert!(refused(TeamCall {
        computer: "-oProxyCommand=x".into(),
        ..call(TeamAction::Pause)
    }));
    assert!(refused(TeamCall {
        request_id: Some("not-a-uuid".into()),
        ..call(TeamAction::Pause)
    }));
    assert!(refused(call(TeamAction::Reject {
        task: 6,
        reason: "  ".into(),
        hold: false,
        fresh: false
    })));
    assert!(refused(call(TeamAction::Accept {
        task: 0,
        reason: String::new(),
        manual: false
    })));
    assert!(refused(call(TeamAction::DraftUpdate {
        revision: 1,
        set: json!({})
    })));
    assert!(refused(call(TeamAction::Settings {
        settings: json!({})
    })));
    assert!(requests(&fake).is_empty(), "no refused request ran");
    // A peer: `hgs @studio team ...`, Qt's 45 s.
    let (invocation, _) = prepare(&TeamCall {
        computer: "studio".into(),
        ..call(TeamAction::Pause)
    })
    .unwrap();
    assert_eq!(invocation.args, ["@studio", "team", "pause", "--json"]);
    assert_eq!(invocation.timeout, Duration::from_secs(45));
    reply(
        &fake,
        "pause",
        "studio",
        json!({"stdout": {"status": "paused"}}),
    );
    let outcome = run(
        &fake.path(),
        &TeamCall {
            computer: "studio".into(),
            ..call(TeamAction::Pause)
        },
    );
    assert!(matches!(outcome, TeamOutcome::Done { .. }), "{outcome:?}");
    assert_eq!(
        fake.log()
            .iter()
            .filter(|entry| entry["host"] == "studio" && entry.get("team").is_some())
            .count(),
        1
    );
}

#[test]
fn the_webview_names_an_action_by_its_kind() {
    let call: TeamCall = serde_json::from_value(json!({
        "computer": "", "team": TEAM, "confirmed": true,
        "action": {"kind": "reject", "task": 6, "reason": "Add the test", "hold": true}
    }))
    .unwrap();
    assert_eq!(
        call.action,
        TeamAction::Reject {
            task: 6,
            reason: "Add the test".into(),
            hold: true,
            fresh: false
        }
    );
    let outcome = serde_json::to_value(TeamOutcome::Uncertain {
        detail: "late".into(),
        request_id: "r".into(),
    })
    .unwrap();
    assert_eq!(
        outcome,
        json!({"outcome": "uncertain", "detail": "late", "request_id": "r"})
    );
    let decide: TeamAction = serde_json::from_value(
        json!({"kind": "decide_proposal", "proposal": "p1", "accept": false, "expected_hash": "h"}),
    )
    .unwrap();
    assert_eq!(decide.op(), "decide-proposal");
    let settings: TeamAction = serde_json::from_value(
        json!({"kind": "settings", "settings": {"land": true}}),
    )
    .unwrap();
    assert_eq!(settings.op(), "settings");
}
