//! Team actions (task #7) against the fake `hgs` (tests/support/fake_hgs.py),
//! never the real CLI: each action's request, its receipt, the owner's
//! confirmation, refusals and unknown outcomes that are never sent again.
mod common;

use common::FakeHgs;
use serde_json::{json, Value};
use std::time::Duration;
use zerus_client_core::teams::action::{classify, prepare, run, Parent, TeamAction, TeamCall, TeamOutcome};
use zerus_client_core::Exit;

const TEAM: &str = "33fbafeb-8291-4eda-a00c-6e1448ec6f20";
const NEW_TEAM: &str = "6d0aabee-1111-4222-8333-444455556666";

fn fake(test: &str) -> FakeHgs {
    FakeHgs::new(test, json!({"": {}, "studio": {}}))
}

fn reply(fake: &FakeHgs, op: &str, host: &str, answer: Value) {
    let name = match op.strip_prefix("catalog ") {
        Some(sub) => format!(
            "team-catalog-{sub}-{}.json",
            if host.is_empty() { "local" } else { host }
        ),
        None => format!(
            "team-{op}-{}.json",
            if host.is_empty() { "local" } else { host }
        ),
    };
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

/// The catalog requests the fake received, in order.
fn catalog_requests(fake: &FakeHgs) -> Vec<(String, Value)> {
    requests(fake)
        .into_iter()
        .filter(|(op, _)| op.starts_with("catalog "))
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
        // Task 11: the answers of Waiting for you.
        (
            TeamAction::Answer {
                member: "backend".into(),
                question_id: "claude-approval:6f2b0e3a".into(),
                expected_question_hash: "6f2b0e3a".into(),
                answers: json!([{"question_id": "approval", "selected_option_ids": ["1"], "text": ""}]),
            },
            "answer",
            json!({"member": "backend", "question_id": "claude-approval:6f2b0e3a", "expected_question_hash": "6f2b0e3a",
                   "answers": [{"question_id": "approval", "selected_option_ids": ["1"], "text": ""}]}),
        ),
        (
            TeamAction::AddToPhase {
                phase: 1,
                confirm: "add-1".into(),
                confirmation: "add".into(),
                decline: false,
                tasks: vec![],
            },
            "add-to-phase",
            json!({"phase": 1, "confirm": "add-1", "confirmation": "add"}),
        ),
        (
            TeamAction::AddToPhase {
                phase: 1,
                confirm: "add-1".into(),
                confirmation: "add".into(),
                decline: true,
                tasks: vec![],
            },
            "add-to-phase",
            json!({"phase": 1, "confirm": "add-1", "decline": true}),
        ),
        // Task 199: the member menu and "+ Member" (Qt TeamWorkspace::memberMenu).
        (TeamAction::PauseMember { member: "api".into() }, "pause-member", json!({"member": "api"})),
        (TeamAction::ResumeMember { member: "api".into() }, "resume-member", json!({"member": "api"})),
        (TeamAction::Relaunch { member: "api".into() }, "relaunch", json!({"member": "api"})),
        (
            TeamAction::Drain { member: "api".into(), role: "Backend".into(), cancel: true },
            "drain",
            json!({"member": "api", "draining": false}),
        ),
        (
            TeamAction::ReportsTo { member: "api".into(), parent: "arch".into() },
            "reports-to",
            json!({"member": "api", "parent": "arch"}),
        ),
        (TeamAction::SubLead { member: "arch".into(), sub: true }, "sub-lead", json!({"member": "arch", "sub": true})),
        (
            TeamAction::Scale { role: " Backend ".into(), agent: "codex".into(), count: 2, model: "gpt-5".into(), effort: String::new(), account: String::new() },
            "scale",
            json!({"role": "Backend", "agent": "codex", "count": 2, "model": "gpt-5"}),
        ),
        (
            TeamAction::SwapMember { member: "api".into(), agent: String::new(), model: "opus".into(), effort: "high".into(), account: String::new(), fresh: false },
            "swap-member",
            json!({"member": "api", "model": "opus", "effort": "high"}),
        ),
        (
            TeamAction::SwapMember { member: "api".into(), agent: String::new(), model: String::new(), effort: String::new(), account: String::new(), fresh: true },
            "swap-member",
            json!({"member": "api", "fresh": true}),
        ),
        (
            TeamAction::MoveAccount { provider: "claude".into(), from_account: "native-claude".into(), to_account: "work".into() },
            "swap-member",
            json!({"provider": "claude", "from_account": "native-claude", "to_account": "work"}),
        ),
        (
            TeamAction::AddMember { member: json!({"id": "qa", "role": "Tester", "agent": "kimi"}) },
            "add-member",
            json!({"members": [{"id": "qa", "role": "Tester", "agent": "kimi"}]}),
        ),
        (
            TeamAction::IntakeKey { label: " watcher ".into(), revoke: String::new() },
            "intake-key",
            json!({"label": "watcher"}),
        ),
        (
            TeamAction::ForceTest { suite: "client-core".into() },
            "test",
            json!({"action": "submit", "suite": "client-core", "forced": true, "asked_by": "You"}),
        ),
        // Task 196: the phase header's actions (Qt TeamWorkspace).
        (
            TeamAction::AddToPhase {
                phase: 2,
                confirm: String::new(),
                confirmation: String::new(),
                decline: false,
                tasks: vec![json!({"title": "Docs", "assignee": "dev"})],
            },
            "add-to-phase",
            json!({"phase": 2, "tasks": [{"title": "Docs", "assignee": "dev"}]}),
        ),
        (
            TeamAction::RejectPhase { phase: 3, comment: " Split it ".into(), revision: Some(14) },
            "reject-phase",
            json!({"phase": 3, "comment": "Split it", "revision": 14}),
        ),
        (
            TeamAction::UndoRejectPhase { phase: 3 },
            "undo-reject-phase",
            json!({"phase": 3}),
        ),
        (
            TeamAction::MovePhase { phase: 3, to: 2, position: "before".into() },
            "move-phase",
            json!({"phase": 3, "to": 2, "position": "before"}),
        ),
        (
            TeamAction::PausePhase { phase: 3, reason: " after the release ".into() },
            "pause-phase",
            json!({"phase": 3, "reason": "after the release"}),
        ),
        (
            TeamAction::UnpausePhase { phase: 3 },
            "unpause-phase",
            json!({"phase": 3}),
        ),
        (
            TeamAction::PhaseAcceptance { phase: 3, owner: true },
            "phase-acceptance",
            json!({"phase": 3, "acceptance": "owner"}),
        ),
        (
            TeamAction::PhaseAcceptance { phase: 3, owner: false },
            "phase-acceptance",
            json!({"phase": 3, "acceptance": "lead"}),
        ),
        (
            TeamAction::EditDraft {
                phase: 3,
                revision: Some(14),
                title: Some(" Ship ".into()),
                barrier: None,
                tasks_update: vec![json!({"id": 9, "deps": [{"task": 8, "kind": "hard"}]})],
            },
            "edit-draft",
            json!({"phase": 3, "revision": 14, "title": "Ship", "tasks_update": [{"id": 9, "deps": [{"task": 8, "kind": "hard"}]}]}),
        ),
        (
            TeamAction::AcceptPhase { phase: 1 },
            "accept-phase",
            json!({"phase": 1}),
        ),
        (
            TeamAction::SendBackPhase {
                phase: 1,
                comment: "Add the migration steps".into(),
            },
            "send-back-phase",
            json!({"phase": 1, "comment": "Add the migration steps"}),
        ),
        (
            TeamAction::Release { task: 7 },
            "release",
            json!({"task": 7}),
        ),
        (TeamAction::Retry { task: 7 }, "retry", json!({"task": 7})),
        (
            TeamAction::Finish {
                summary: " Shipped the API. ".into(),
            },
            "finish",
            json!({"summary": "Shipped the API."}),
        ),
        // Task 40: the team bar's Discard plan and the task card's actions.
        (TeamAction::Discard, "discard", json!({})),
        (
            TeamAction::Hold {
                task: 6,
                reason: "After the release".into(),
            },
            "hold",
            json!({"task": 6, "reason": "After the release"}),
        ),
        (
            TeamAction::SkipStage {
                task: 6,
                reason: "Reviewed by hand".into(),
            },
            "skip-stage",
            json!({"task": 6, "reason": "Reviewed by hand"}),
        ),
        (
            TeamAction::SendBack {
                task: 6,
                reason: "Paging is missing".into(),
            },
            "send-back",
            json!({"task": 6, "reason": "Paging is missing"}),
        ),
        (
            TeamAction::MoveTask {
                task: 6,
                phase: 2,
                revision: Some(9),
            },
            "move-task",
            json!({"task": 6, "phase": 2, "revision": 9}),
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
                members_update: vec![],
            },
            "settings",
            json!({"settings": {"branch_per_task": true, "land": false}}),
        ),
        (
            TeamAction::Settings {
                settings: json!({"build_broker": true}),
                members_update: vec![json!({"id": "dev", "iron_rules": ["Run the tests"]})],
            },
            "settings",
            json!({"settings": {"build_broker": true}, "members_update": [{"id": "dev", "iron_rules": ["Run the tests"]}]}),
        ),
        (
            TeamAction::Finding {
                finding: "f1".into(), action: "fixed".into(), note: None, proposal: None, task: Some(4),
            },
            "finding",
            json!({"finding": "f1", "action": "fixed", "task": 4}),
        ),
        (
            TeamAction::NextGoal { goal: "Ship the next goal".into(), name: Some("Orbit 2".into()) },
            "next-goal",
            json!({"goal": "Ship the next goal", "name": "Orbit 2"}),
        ),
        (
            TeamAction::Create { name: "child".into(), parent: Parent { team: TEAM.into(), task: 4 } },
            "create",
            json!({"name": "child", "parent": {"team": TEAM, "task": 4}}),
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
    // Task 11: a tool offer belongs to no team; Deny sends its reason, Allow none.
    for (allow, fields) in [
        (
            false,
            json!({"offer": "a1", "allow": false, "reason": "Not on this laptop"}),
        ),
        (true, json!({"offer": "a1", "allow": true})),
    ] {
        reply(
            &fake,
            "decide-tool",
            "",
            json!({"stdout": {"state": "denied"}}),
        );
        let tool = TeamAction::DecideTool {
            offer: "a1".into(),
            allow,
            reason: "Not on this laptop".into(),
        };
        let outcome = run(
            &fake.path(),
            &TeamCall {
                team: String::new(),
                ..call(tool)
            },
        );
        let TeamOutcome::Done { request_id, .. } = &outcome else {
            panic!("{outcome:?}")
        };
        let (op, request) = requests(&fake).pop().unwrap();
        assert_eq!(op, "decide-tool");
        let mut expected = fields.as_object().unwrap().clone();
        expected.insert("by".into(), json!("human"));
        expected.insert("request_id".into(), json!(request_id));
        assert_eq!(request, Value::Object(expected));
    }
    // Task 97: a deferred request belongs to no team; the owner's answer is
    // sent as it is, and an answer without words never runs.
    reply(&fake, "answer-deferred", "", json!({"stdout": {"status": "sent"}}));
    let outcome = run(
        &fake.path(),
        &TeamCall {
            team: String::new(),
            ..call(TeamAction::AnswerDeferred { id: "0a1b2c3d4e5f6a7b".into(), decision: "allow".into(), text: String::new() })
        },
    );
    let TeamOutcome::Done { request_id, .. } = &outcome else { panic!("{outcome:?}") };
    let (op, request) = requests(&fake).pop().unwrap();
    assert_eq!(op, "answer-deferred");
    assert_eq!(request, json!({"id":"0a1b2c3d4e5f6a7b","decision":"allow","by":"human","request_id":request_id}));
    let before_deferred = requests(&fake).len();
    let empty = run(
        &fake.path(),
        &TeamCall {
            team: String::new(),
            ..call(TeamAction::AnswerDeferred { id: "0a1b2c3d4e5f6a7b".into(), decision: "answer".into(), text: " ".into() })
        },
    );
    assert!(matches!(empty, TeamOutcome::Failed { .. }), "{empty:?}");
    assert_eq!(requests(&fake).len(), before_deferred);
    // Task 108: the Disk view belongs to no team. A list and a dry run go at
    // once; Clean now asks first, then sends exactly what was confirmed.
    let disk = |action: &str, dry_run: bool| TeamAction::Disk {
        action: action.into(), scope: "policy".into(), dry_run, policy: Value::Null, paths: vec![], path: String::new(), refresh: false, measure: false,
    };
    reply(&fake, "disk", "", json!({"stdout": {"decisions": []}}));
    let listed = run(&fake.path(), &TeamCall { team: String::new(), ..call(disk("list", false)) });
    assert!(matches!(listed, TeamOutcome::Done { .. }), "{listed:?}");
    assert_eq!(requests(&fake).pop().unwrap().1["action"], "list");
    // Task 109: the open Disk view's live read asks hgs to measure.
    let live = run(&fake.path(), &TeamCall { team: String::new(), ..call(TeamAction::Disk {
        action: "list".into(), scope: String::new(), dry_run: false, policy: Value::Null, paths: vec![], path: String::new(), refresh: false, measure: true,
    }) });
    assert!(matches!(live, TeamOutcome::Done { .. }), "{live:?}");
    assert_eq!(requests(&fake).pop().unwrap().1["measure"], true);
    let dry = run(&fake.path(), &TeamCall { team: String::new(), ..call(disk("clean", true)) });
    assert!(matches!(dry, TeamOutcome::Done { .. }), "{dry:?}");
    assert_eq!(requests(&fake).pop().unwrap().1["dry_run"], true);
    let before_clean = requests(&fake).len();
    let asked = run(&fake.path(), &TeamCall { team: String::new(), confirmed: false, ..call(disk("clean", false)) });
    let TeamOutcome::Confirm { confirmation } = &asked else { panic!("{asked:?}") };
    assert_eq!((confirmation.name.as_str(), confirmation.action.as_str()), ("diskCleanConfirm", "Remove"));
    assert_eq!(requests(&fake).len(), before_clean, "nothing runs before the owner confirms");
    let cleaned = run(&fake.path(), &TeamCall { team: String::new(), confirmed: true, ..call(disk("clean", false)) });
    assert!(matches!(cleaned, TeamOutcome::Done { .. }), "{cleaned:?}");
    let (op, request) = requests(&fake).pop().unwrap();
    assert_eq!((op.as_str(), request["dry_run"].clone(), request["scope"].clone(), request["by"].clone()), ("disk", json!(false), json!("policy"), json!("human")));
    let wrong = run(&fake.path(), &TeamCall { team: String::new(), ..call(TeamAction::Disk { action: "wipe".into(), scope: String::new(), dry_run: true,
        policy: Value::Null, paths: vec![], path: String::new(), refresh: false, measure: false }) });
    assert!(matches!(wrong, TeamOutcome::Failed { .. }), "{wrong:?}");
    // A send back without a comment and an answer without a choice never run.
    let before = requests(&fake).len();
    for action in [
        TeamAction::SendBackPhase {
            phase: 1,
            comment: "  ".into(),
        },
        TeamAction::Answer {
            member: "backend".into(),
            question_id: "q".into(),
            expected_question_hash: String::new(),
            answers: json!([]),
        },
    ] {
        let outcome = run(&fake.path(), &call(action));
        assert!(matches!(outcome, TeamOutcome::Failed { .. }), "{outcome:?}");
    }
    assert_eq!(requests(&fake).len(), before);
}

#[test]
fn catalog_actions_send_their_requests_and_apply_and_discard_ask_first() {
    let fake = fake("team_actions_catalog");
    let cases = [
        (
            TeamAction::CatalogPropose {
                change: json!({"kind": "role", "target": "tester", "action": "update"}),
            },
            "propose",
            json!({"change": {"kind": "role", "target": "tester", "action": "update"}}),
        ),
        (
            TeamAction::CatalogDecide {
                proposal: "p1".into(),
                decision: "keep".into(),
            },
            "decide",
            json!({"proposal": "p1", "decision": "keep"}),
        ),
        (TeamAction::CatalogApply, "apply", json!({})),
        (TeamAction::CatalogDiscard, "discard", json!({})),
    ];
    let mut sent = 0;
    for (action, sub, fields) in cases {
        reply(
            &fake,
            &format!("catalog {sub}"),
            "",
            json!({"stdout": {"status": "ok"}, "echo": true}),
        );
        let outcome = run(
            &fake.path(),
            &TeamCall {
                team: String::new(),
                ..call(action)
            },
        );
        let TeamOutcome::Done {
            receipt,
            request_id,
        } = &outcome
        else {
            panic!("{sub}: {outcome:?}")
        };
        assert_eq!(receipt["request_id"], json!(request_id), "{sub}");
        sent += 1;
        let log = catalog_requests(&fake);
        assert_eq!(log.len(), sent, "{sub} ran once");
        let (logged_op, request) = log.last().unwrap();
        assert_eq!(logged_op, &format!("catalog {sub}"));
        let mut expected = fields.as_object().unwrap().clone();
        expected.insert("by".into(), json!("human"));
        expected.insert("request_id".into(), json!(request_id));
        assert_eq!(request, &Value::Object(expected), "{sub}");
        assert!(
            request.get("team").is_none(),
            "a catalog call names no team"
        );
    }
    // Apply and discard end the draft, so both ask the owner first.
    for (action, dialog) in [
        (TeamAction::CatalogApply, "teamCatalogApplyConfirm"),
        (TeamAction::CatalogDiscard, "teamCatalogDiscardConfirm"),
    ] {
        let outcome = run(
            &fake.path(),
            &TeamCall {
                team: String::new(),
                confirmed: false,
                ..call(action)
            },
        );
        let TeamOutcome::Confirm { confirmation } = &outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(confirmation.name, dialog);
    }
    assert_eq!(
        catalog_requests(&fake).len(),
        sent,
        "nothing ran before confirmation"
    );
    // The exact command is `hgs team catalog decide --json`.
    let (invocation, _) = prepare(&TeamCall {
        team: String::new(),
        ..call(TeamAction::CatalogDecide {
            proposal: "p1".into(),
            decision: "keep".into(),
        })
    })
    .unwrap();
    assert_eq!(invocation.args, ["team", "catalog", "decide", "--json"]);
    // hgs's own refusal is failed, never uncertain or resent.
    reply(
        &fake,
        "catalog decide",
        "",
        json!({"exit": 1, "stderr": "hgs: only the owner decides catalog proposals\n"}),
    );
    let outcome = run(
        &fake.path(),
        &TeamCall {
            team: String::new(),
            ..call(TeamAction::CatalogDecide {
                proposal: "p1".into(),
                decision: "keep".into(),
            })
        },
    );
    assert!(
        matches!(&outcome, TeamOutcome::Failed { detail, .. } if detail == "hgs: only the owner decides catalog proposals"),
        "{outcome:?}"
    );
}

#[test]
fn destructive_actions_and_approvals_ask_first_and_send_nothing() {
    let fake = fake("team_actions_confirm");
    for action in [
        TeamAction::Stop,
        TeamAction::Finish {
            summary: "Done".into(),
        },
        TeamAction::Discard,
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
        TeamAction::DiscardPhase { phase: 3, tasks: 4, accepted: false },
        TeamAction::ReturnToDraft { phase: 3 },
        TeamAction::Drain { member: "api".into(), role: "Backend".into(), cancel: false },
        TeamAction::RemoveMember { member: "api".into(), role: "Backend".into() },
        TeamAction::IntakeKey { label: "watcher".into(), revoke: "k1".into() },
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
        members_update: vec![],
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
        settings: json!({}),
        members_update: vec![],
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
    let settings: TeamAction =
        serde_json::from_value(json!({"kind": "settings", "settings": {"land": true}})).unwrap();
    assert_eq!(settings.op(), "settings");
    let finding: TeamAction = serde_json::from_value(json!({
        "kind": "finding", "finding": "f1", "action": "replan", "proposal": "Split phase 2"
    }))
    .unwrap();
    assert_eq!(finding.op(), "finding");
    let goal: TeamAction =
        serde_json::from_value(json!({"kind": "next_goal", "goal": "Next", "name": "Orbit 2"})).unwrap();
    assert_eq!(goal.op(), "next-goal");
    let create: TeamAction = serde_json::from_value(json!({
        "kind": "create", "name": "child", "parent": {"team": TEAM, "task": 4}
    }))
    .unwrap();
    assert_eq!(create.op(), "create");
}

/// Task #44: Qt's catalog editor in core. The owner edits a field, exports
/// the working copy, imports it into another editor and saves: `hgs team
/// catalog set` gets the catalog over the version it was read at, with
/// `by: human`, once. The designer's start sends the project's folder, the
/// first message as its goal and the chosen agent.
#[test]
fn catalog_set_import_export_and_designer_send_their_requests() {
    use zerus_client_core::teams::catalog_edit::{CatalogEdit, CatalogEditor, Kind};
    let fake = fake("team_actions_catalog_edit");
    let get = json!({"builtin":false,"draft":{"base":4,"proposals":[]},"catalog":{"version":4,
        "roles":[{"id":"backend","name":"Backend","agent":"codex","prompt":"Build it."}],
        "templates":[{"id":"feature","name":"Feature","roster":[{"role":"backend","count":1}],"tasks":[]}],
        "routing":[],"pipelines":[],"planning_rules":"(1) small phases"}});
    let mut editor = CatalogEditor::default();
    editor.load(&get).unwrap();
    editor
        .edit(&CatalogEdit::Set { kind: Kind::Role, id: "backend".into(), field: "about".into(), value: json!("The API") })
        .unwrap();
    // Export writes the working copy; Import reads it into another editor.
    let file = editor.export_text();
    let mut imported = CatalogEditor::default();
    imported.load(&get).unwrap();
    imported.import_text(&file).unwrap();
    assert_eq!(imported.working()["roles"][0]["about"], "The API");
    reply(&fake, "catalog set", "", json!({"stdout": {"status": "saved", "version": 5}, "echo": true}));
    let outcome = run(&fake.path(), &TeamCall { team: String::new(), ..call(imported.save_action()) });
    let TeamOutcome::Done { receipt, request_id } = &outcome else { panic!("{outcome:?}") };
    assert_eq!(receipt["status"], "saved");
    let log = catalog_requests(&fake);
    assert_eq!(log.len(), 1, "saved once");
    let (op, request) = &log[0];
    assert_eq!(op, "catalog set");
    assert_eq!(request["version"], 4, "over the version it was read at");
    assert_eq!(request["catalog"]["roles"][0]["about"], "The API");
    assert_eq!(request["catalog"]["planning_rules"], "(1) small phases");
    assert_eq!((request["by"].as_str(), request["request_id"].as_str()), (Some("human"), Some(request_id.as_str())));
    // A newer catalog refuses the save: nothing was written, and the
    // refusal is the owner's to read.
    reply(&fake, "catalog set", "", json!({"stderr": "hgs: The catalog changed (now v5); refresh and try again", "exit": 1}));
    let refused = run(&fake.path(), &TeamCall { team: String::new(), ..call(imported.save_action()) });
    let TeamOutcome::Failed { detail, .. } = &refused else { panic!("{refused:?}") };
    assert!(detail.contains("catalog changed"), "{detail}");
    // The designer's conversation starts for the selected project.
    reply(&fake, "catalog designer", "", json!({"stdout": {"status": "started", "name": "claude/designer/team-designer"}, "echo": true}));
    let designer = TeamAction::CatalogDesigner {
        folder: "C:/work/orbit".into(),
        goal: "A team for the payments service".into(),
        agent: "claude".into(),
        model: "opus".into(),
        effort: String::new(),
        project: "Orbit".into(),
    };
    let started = run(&fake.path(), &TeamCall { team: String::new(), ..call(designer) });
    assert!(matches!(started, TeamOutcome::Done { .. }), "{started:?}");
    let (op, request) = catalog_requests(&fake).last().unwrap().clone();
    assert_eq!(op, "catalog designer");
    assert_eq!(request["folder"], "C:/work/orbit");
    assert_eq!(request["goal"], "A team for the payments service");
    assert_eq!((request["agent"].as_str(), request["model"].as_str(), request["project"].as_str()), (Some("claude"), Some("opus"), Some("Orbit")));
    assert!(request.get("effort").is_none(), "an empty choice is not sent");
    // Without a folder nothing starts.
    let none = TeamAction::CatalogDesigner { folder: String::new(), goal: "x".into(), agent: String::new(), model: String::new(), effort: String::new(), project: String::new() };
    assert!(matches!(run(&fake.path(), &TeamCall { team: String::new(), ..call(none) }), TeamOutcome::Failed { .. }));
}

/// Task #58: the owner switches one phase's mode: the confirmation first
/// (Qt's dialog words), then exactly `hgs team phase-mode --json` with the
/// phase, the mode and the owner's identity.
#[test]
fn a_phase_mode_switch_asks_first_then_sends_the_phase_and_its_mode() {
    let switch = |mode: &str| TeamAction::PhaseMode { phase: 2, mode: mode.into(), reason: " release ".into() };
    let Err(TeamOutcome::Confirm { confirmation }) = prepare(&TeamCall { confirmed: false, ..call(switch("free")) }) else { panic!("asks first") };
    assert_eq!((confirmation.name.as_str(), confirmation.action.as_str()), ("teamPhaseModeConfirm", "Switch to free"));
    assert!(confirmation.text.contains("phase 2") && confirmation.text.contains("other phases keep their modes"), "{}", confirmation.text);
    let Err(TeamOutcome::Confirm { confirmation }) = prepare(&TeamCall { confirmed: false, ..call(switch("standard")) }) else { panic!("asks first") };
    assert_eq!(confirmation.action, "Switch to standard");
    let (invocation, request_id) = prepare(&call(switch("standard"))).unwrap();
    assert_eq!(invocation.args, ["team", "phase-mode", "--json"]);
    let sent: Value = serde_json::from_slice(invocation.stdin.as_deref().unwrap()).unwrap();
    assert_eq!(sent, json!({"phase":2,"mode":"standard","reason":"release","team":TEAM,"by":"human","request_id":request_id}));
    for bad in [TeamAction::PhaseMode { phase: 0, mode: "free".into(), reason: String::new() },
                TeamAction::PhaseMode { phase: 2, mode: "open".into(), reason: String::new() }] {
        assert!(matches!(prepare(&call(bad)), Err(TeamOutcome::Failed { .. })));
    }
}

/// Task #12: the team's own mode is the same `hgs team phase-mode`, without
/// a phase; it asks first unless the owner already confirmed it.
#[test]
fn a_team_mode_switch_sends_the_mode_without_a_phase() {
    let switch = |mode: &str| TeamAction::TeamMode {
        mode: mode.into(),
        reason: String::new(),
    };
    let Err(TeamOutcome::Confirm { confirmation }) = prepare(&TeamCall {
        confirmed: false,
        ..call(switch("free"))
    }) else {
        panic!("asks first")
    };
    assert_eq!(
        (confirmation.name.as_str(), confirmation.action.as_str()),
        ("teamModeConfirm", "Switch to free")
    );
    assert!(
        confirmation.text.contains("own mode keeps it"),
        "{}",
        confirmation.text
    );
    let (invocation, request_id) = prepare(&call(switch("free"))).unwrap();
    assert_eq!(invocation.args, ["team", "phase-mode", "--json"]);
    let sent: Value = serde_json::from_slice(invocation.stdin.as_deref().unwrap()).unwrap();
    assert_eq!(
        sent,
        json!({"mode":"free","team":TEAM,"by":"human","request_id":request_id})
    );
    assert!(sent.get("phase").is_none());
    assert!(matches!(
        prepare(&call(switch("open"))),
        Err(TeamOutcome::Failed { .. })
    ));
    let parsed: TeamAction =
        serde_json::from_value(json!({"kind":"team_mode","mode":"standard"})).unwrap();
    assert!(matches!(parsed, TeamAction::TeamMode { ref mode, .. } if mode == "standard"));
}

/// Task 88: the Tests settings read and check the team's test configuration;
/// neither is a change (no confirmation), a candidate override goes as it is,
/// and anything but an object or null is refused before anything runs.
#[test]
fn the_test_config_action_reads_and_checks_without_asking() {
    let get = TeamAction::TestConfig { action: "get".into(), tests: Value::Null };
    let (invocation, _) = prepare(&call(get.clone())).unwrap();
    assert_eq!(&invocation.args[..3], ["team", "test-config", "--json"]);
    let sent: Value = serde_json::from_slice(invocation.stdin.as_deref().unwrap()).unwrap();
    assert_eq!((sent["action"].clone(), sent["team"].clone(), sent.get("tests").is_none()), (json!("get"), json!(TEAM), true));
    assert!(get.confirmation("T").is_none());
    let candidate = json!({"provider":[{"name":"python","kind":"pytest","results":{"format":"junit","path":"out/pytest.xml"}}]});
    let check = TeamAction::TestConfig { action: "check".into(), tests: candidate.clone() };
    let (invocation, _) = prepare(&TeamCall { confirmed: false, ..call(check) }).unwrap();
    let sent: Value = serde_json::from_slice(invocation.stdin.as_deref().unwrap()).unwrap();
    assert_eq!(sent["tests"], candidate);
    for wrong in [TeamAction::TestConfig { action: "write".into(), tests: Value::Null }, TeamAction::TestConfig { action: "check".into(), tests: json!([1]) }] {
        assert!(matches!(prepare(&call(wrong)), Err(TeamOutcome::Failed { .. })));
    }
    // From the webview's JSON.
    let parsed: TeamAction = serde_json::from_value(json!({"kind":"test_config","action":"check","tests":{"provider":[]}})).unwrap();
    assert!(matches!(parsed, TeamAction::TestConfig { .. }));
}

/// Task 88: a run's report reaches the Tests mode bounded: every count, at
/// most 200 results, each text cut.
#[test]
fn a_test_runs_report_reaches_the_tests_mode_bounded() {
    let results: Vec<Value> = (0..250).map(|n| json!({"suite":"math","name":format!("t{n}"),"status":if n == 0 { "failed" } else { "passed" },"ms":n,"message":"x".repeat(5000)})).collect();
    let view = zerus_client_core::teams::view::tests(
        "t",
        &json!({"items":[]}),
        &json!({"items":[{"id":"r1","suite":"engine","status":"failed","report":{"passed":249,"failed":1,"skipped":0,"errors":0,"note":"","results":results}},
                         {"id":"r2","suite":"plain","status":"passed"}]}),
    );
    let report = &view.runs[0].report;
    assert_eq!((report["passed"].clone(), report["failed"].clone()), (json!(249), json!(1)));
    assert_eq!(report["results"].as_array().unwrap().len(), 200);
    assert_eq!(report["results"][0]["status"], "failed");
    assert!(report["results"][0]["message"].as_str().unwrap().chars().count() < 5000);
    assert!(view.runs[1].report.is_null());
}

/// Task 196: Discard phase and Return to draft ask in Qt's words (an accepted
/// phase says so), then send only the phase; bad phase numbers, moves, pause
/// reasons and new tasks are refused before anything runs.
#[test]
fn phase_actions_ask_in_qt_words_and_refuse_bad_requests() {
    let ask = |action: TeamAction| match prepare(&TeamCall { confirmed: false, ..call(action) }) {
        Err(TeamOutcome::Confirm { confirmation }) => confirmation,
        other => panic!("asks first: {other:?}"),
    };
    let draft = ask(TeamAction::DiscardPhase { phase: 3, tasks: 4, accepted: false });
    assert_eq!((draft.name.as_str(), draft.title.as_str(), draft.action.as_str()), ("teamDiscardPhaseConfirm", "Discard phase?", "Discard phase"));
    assert_eq!(draft.text, "Discard phase 3 and its 4 tasks?");
    let accepted = ask(TeamAction::DiscardPhase { phase: 3, tasks: 4, accepted: true });
    assert!(accepted.text.starts_with("Phase 3 is already accepted.") && accepted.text.contains("cannot be undone"), "{}", accepted.text);
    let back = ask(TeamAction::ReturnToDraft { phase: 3 });
    assert_eq!((back.name.as_str(), back.action.as_str()), ("teamReturnPhaseConfirm", "Return to draft"));
    assert!(back.text.contains("Phase 3 goes back to a draft"), "{}", back.text);
    let (invocation, request_id) = prepare(&call(TeamAction::DiscardPhase { phase: 3, tasks: 4, accepted: true })).unwrap();
    assert_eq!(invocation.args, ["team", "discard-phase", "--json"]);
    let sent: Value = serde_json::from_slice(invocation.stdin.as_deref().unwrap()).unwrap();
    assert_eq!(sent, json!({"phase": 3, "team": TEAM, "by": "human", "request_id": request_id}));
    let long = "x".repeat(401);
    for bad in [
        TeamAction::RejectPhase { phase: 3, comment: "  ".into(), revision: None },
        TeamAction::RejectPhase { phase: 0, comment: "why".into(), revision: None },
        TeamAction::UndoRejectPhase { phase: 0 },
        TeamAction::MovePhase { phase: 3, to: 3, position: "before".into() },
        TeamAction::MovePhase { phase: 3, to: 2, position: "under".into() },
        TeamAction::PausePhase { phase: 3, reason: long },
        TeamAction::AddToPhase { phase: 2, confirm: String::new(), confirmation: String::new(), decline: false, tasks: vec![json!({"title": ""})] },
        TeamAction::AddToPhase { phase: 2, confirm: String::new(), confirmation: String::new(), decline: false, tasks: (0..21).map(|n| json!({"title": format!("T{n}")})).collect() },
    ] {
        assert!(matches!(prepare(&call(bad.clone())), Err(TeamOutcome::Failed { .. })), "{bad:?}");
    }
}

/// Task 196: an edit of the plan while the team plans reads the team, sends
/// the whole plan (members without the lead, tasks without cancelled ones, the
/// change applied) at the revision the owner saw, and sends nothing when the
/// team no longer plans or cannot be read.
#[test]
fn a_plan_edit_sends_the_whole_plan_with_the_change_at_the_owners_revision() {
    use zerus_client_core::teams::plan_edit::PlanChange;
    let fake = fake("team_actions_plan_edit");
    let status = json!({
        "status": "planning", "lead": "lead", "revision": 7,
        "members": [
            {"id": "lead", "role": "Lead", "agent": "claude", "lead": true},
            {"id": "dev", "role": "Developer", "agent": "codex", "model": "gpt", "secret_runtime": "no"}
        ],
        "tasks": [
            {"id": 1, "phase": 1, "title": "Core", "assignee": "dev", "deps": [], "status": "draft", "estimate_minutes": 30, "estimate_source": "lead"},
            {"id": 2, "phase": 1, "title": "UI", "assignee": "dev", "deps": [], "status": "draft", "estimate_minutes": 20, "estimate_source": "auto"},
            {"id": 3, "phase": 2, "title": "Old", "assignee": "dev", "deps": [], "status": "cancelled"}
        ],
        "phases": [{"n": 2, "title": "Polish", "barrier": true}]
    });
    reply(&fake, "status", "", json!({"stdout": status}));
    reply(&fake, "plan", "", json!({"stdout": {"status": "ok"}}));
    let edit = |change: PlanChange| call(TeamAction::PlanEdit { revision: 7, change });
    let link: PlanChange = serde_json::from_value(json!({"edit": "link", "dependency": 1, "task": 2, "add": true})).unwrap();
    let outcome = run(&fake.path(), &edit(link));
    let TeamOutcome::Done { request_id, .. } = &outcome else { panic!("{outcome:?}") };
    let log = requests(&fake);
    assert_eq!(log.iter().map(|(op, _)| op.as_str()).collect::<Vec<_>>(), ["status", "plan"]);
    let plan = &log[1].1;
    assert_eq!(plan["revision"], json!(7));
    assert_eq!(plan["request_id"], json!(request_id));
    assert_eq!(plan["members"], json!([{"id": "dev", "role": "Developer", "agent": "codex", "model": "gpt"}]));
    let tasks = plan["tasks"].as_array().unwrap();
    assert_eq!(tasks.iter().map(|task| task["id"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2], "the cancelled task is not in the plan");
    assert_eq!(tasks[1]["deps"], json!([{"task": 1, "kind": "hard"}]));
    assert_eq!(tasks[0]["estimate_minutes"], json!(30), "the lead's estimate stays");
    assert!(tasks[1].get("estimate_minutes").is_none(), "an automatic estimate is not the lead's");
    assert_eq!(plan["phases"], json!([{"n": 2, "title": "Polish", "barrier": true}]));
    // A new task takes the next id over every task, the cancelled ones too.
    let new: PlanChange = serde_json::from_value(json!({"edit": "task", "task": {"title": "Docs", "assignee": "dev", "phase": 2}})).unwrap();
    assert!(matches!(run(&fake.path(), &edit(new)), TeamOutcome::Done { .. }));
    let plan = &requests(&fake)[3].1;
    let added = plan["tasks"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((added["id"].clone(), added["title"].clone(), added["phase"].clone()), (json!(4), json!("Docs"), json!(2)));
    // A team that no longer plans sends nothing.
    reply(&fake, "status", "", json!({"stdout": {"status": "running", "members": [], "tasks": []}}));
    let remove: PlanChange = serde_json::from_value(json!({"edit": "remove_task", "task": 1})).unwrap();
    let outcome = run(&fake.path(), &edit(remove));
    assert!(matches!(&outcome, TeamOutcome::Failed { detail, .. } if detail.contains("no longer plans")), "{outcome:?}");
    assert_eq!(requests(&fake).len(), 5, "only the read ran");
}

/// Task 199: Drain and Remove ask in Qt's words (Remove names the team),
/// then send only the member; bad member requests never run.
#[test]
fn member_actions_ask_in_qt_words_and_refuse_bad_requests() {
    let ask = |action: TeamAction| match prepare(&TeamCall { confirmed: false, ..call(action) }) {
        Err(TeamOutcome::Confirm { confirmation }) => confirmation,
        other => panic!("asks first: {other:?}"),
    };
    let drain = ask(TeamAction::Drain { member: "api".into(), role: "Backend".into(), cancel: false });
    assert_eq!((drain.name.as_str(), drain.title.as_str(), drain.action.as_str()), ("teamDrainMemberConfirm", "Drain member?", "Drain"));
    assert!(drain.text.starts_with("Backend finishes its current task and review"), "{}", drain.text);
    let remove = ask(TeamAction::RemoveMember { member: "api".into(), role: "Backend".into() });
    assert_eq!((remove.name.as_str(), remove.action.as_str()), ("teamRemoveMemberConfirm", "Remove"));
    assert_eq!(remove.text, "Remove Backend from Zerus Next for the fork? Its session keeps running; it gets no more tasks.");
    // Cancelling a drain asks nothing.
    assert!(prepare(&TeamCall { confirmed: false, ..call(TeamAction::Drain { member: "api".into(), role: String::new(), cancel: true }) }).is_ok());
    let (invocation, request_id) = prepare(&call(TeamAction::RemoveMember { member: "api".into(), role: "Backend".into() })).unwrap();
    assert_eq!(invocation.args, ["team", "remove-member", "--json"]);
    let sent: Value = serde_json::from_slice(invocation.stdin.as_deref().unwrap()).unwrap();
    assert_eq!(sent, json!({"member": "api", "team": TEAM, "by": "human", "request_id": request_id}));
    let empty = || String::new();
    for bad in [
        TeamAction::PauseMember { member: empty() },
        TeamAction::ReportsTo { member: "api".into(), parent: "api".into() },
        TeamAction::Scale { role: "Backend".into(), agent: "codex".into(), count: 0, model: empty(), effort: empty(), account: empty() },
        TeamAction::Scale { role: " ".into(), agent: "codex".into(), count: 1, model: empty(), effort: empty(), account: empty() },
        TeamAction::SwapMember { member: "api".into(), agent: empty(), model: empty(), effort: empty(), account: empty(), fresh: false },
        TeamAction::MoveAccount { provider: "claude".into(), from_account: "work".into(), to_account: "work".into() },
        TeamAction::AddMember { member: json!({"role": "Tester"}) },
        TeamAction::AddMember { member: json!("qa") },
    ] {
        assert!(matches!(prepare(&call(bad.clone())), Err(TeamOutcome::Failed { .. })), "{bad:?}");
    }
}
