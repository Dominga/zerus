//! The hook writer's own record of a child, in every shape an end takes
//! (zerus-63rru.78). Each record starts as the supervisor and its main
//! SessionStart hook build it, the child events go through the production
//! writer (`telemetry::observe` then `journal::update_activity`, the order the
//! hook entry uses), and each row is made by the listing's own steps. The test
//! prints rows and the child evidence; its parent reads them with the actor's
//! facts, so nothing here hand-writes a count.
use super::*;
use crate::state::listing_fixture::{confirmed, launched_record, listed_live_row};
use serde_json::json;
use std::collections::BTreeSet;
use std::process::{Command, Stdio};
use std::time::Duration;

fn record(agent: &str) -> Value {
    let mut record = launched_record(
        agent,
        &format!("{agent}/example/member"),
        "00000000-0000-4000-8000-0000000000a1",
        "00000000-0000-4000-8000-0000000000a2",
        &format!("run-{agent}"),
        "/",
        "account-example",
    );
    confirmed(&mut record, &format!("conversation-{agent}"));
    record
}

fn apply(record: &mut Value, events: &[Value]) {
    for event in events {
        telemetry::observe(record, event);
        journal::update_activity(record, event);
    }
}

fn row(record: Value) -> Value {
    listed_live_row(record).expect("a listed row")
}

/// Claude Code names a child by `agent_id` and its `agent_type`.
fn claude(kind: &str, id: &str) -> Value {
    json!({"hook_event_name":kind,"agent_id":id,"agent_type":"general-purpose",
        "last_assistant_message":"the child's result","error_message":"the child failed"})
}

/// A provider that names its children only by agent ID.
fn codex(kind: &str, id: &str) -> Value {
    json!({"hook_event_name":kind,"agent_id":id,"error_message":"the child failed"})
}

/// Kimi's own hooks name a profile, never a child's identity.
fn kimi(kind: &str, profile: &str) -> Value {
    json!({"hook_event_name":kind,"agent_id":"main","agent_name":profile})
}

/// A child whose external process is ended by a verified exit, then its late
/// Start. The process is this test's own direct child.
fn external_process_end() -> (Value, Value, Value) {
    const ID: &str = "external:codex:22222222-2222-4222-8222-222222222222";
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("a sleeping child");
    let mut record = record("claude");
    let pid = std::process::id();
    record["pid"] = json!(pid);
    record["process_start"] = json!(storage::process_start(pid));
    record["subagents"][ID] = json!({"external":true,"provider":"codex","conversation_id":"child",
        "name":"external child","state":"working","started":1.0,"updated":2.0,
        "reply_id":"original-reply","attention_id":"original-attention"});
    record["subagents"][ID]["original_process"] =
        subagent_lifecycle::capture(&record, child.id()).expect("the child's process identity");
    child.kill().expect("kill the child");
    child.wait().expect("reap the child");
    subagent_lifecycle::reconcile(&mut record);
    let ended = record["subagents"][ID].clone();
    apply(&mut record, &[claude("SubagentStart", ID)]);
    let listed = row(record.clone());
    (listed, ended, record["subagents"][ID].clone())
}

/// A Kimi native child removed from its catalog, then a synthetic late Start
/// (Kimi's own hooks never name a native ID; the shape shows what the writer
/// does with a state that is not an end).
fn kimi_catalog_removed() -> (Value, Value, Value) {
    const PARENT: &str = "11111111-1111-4111-8111-111111111111";
    const ID: &str = "agent-0";
    let temp = tempfile::tempdir().expect("a temporary folder");
    let home = temp.path().join("kimi");
    let conversation = format!("session_{PARENT}");
    let session = home.join("sessions/workspace").join(&conversation);
    for id in ["main", ID] {
        std::fs::create_dir_all(session.join("agents").join(id)).expect("agent folder");
    }
    std::fs::write(session.join("agents/main/wire.jsonl"), "{}\n").expect("main wire");
    let agents = json!({"main":{"homedir":session.join("agents/main")},
        ID:{"homedir":session.join("agents").join(ID),"parentAgentId":"main",
            "labels":{"profileName":"code"}}});
    let mut state = json!({"id":conversation,"version":2,"updatedAt":20000,"agents":agents});
    std::fs::write(session.join("state.json"), state.to_string()).expect("catalog");
    let started = json!({"type":"agent.turn.started","agentId":ID,"turnId":0,"time":10000});
    std::fs::write(
        session.join("agents").join(ID).join("wire.jsonl"),
        format!("{started}\n"),
    )
    .expect("wire");
    let mut record = record("kimi");
    record["agent_home"] = json!(home);
    record["conversation_id"] = json!(conversation);
    native_subagents::reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["state"], "working");
    state["agents"].as_object_mut().unwrap().remove(ID);
    std::fs::write(session.join("state.json"), state.to_string()).expect("catalog");
    native_subagents::reconcile(&mut record);
    let removed = record["subagents"][ID].clone();
    // Kimi's own hooks never name a native ID; this raw Start shows what the
    // writer does with the catalog's `unknown`, which is not an end.
    apply(
        &mut record,
        &[json!({"hook_event_name":"SubagentStart","agent_id":ID})],
    );
    let listed = row(record.clone());
    (listed, removed, record["subagents"][ID].clone())
}

/// A live external child whose parent binding is stale: process uncertainty
/// (not an end), then a fresh routed Start, with or without the hook entry's
/// following reconcile.
fn external_process_uncertain(reconciled: bool) -> (Value, Value) {
    const ID: &str = "external:codex:22222222-2222-4222-8222-222222222222";
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("a sleeping child");
    let mut record = record("claude");
    let pid = std::process::id();
    record["pid"] = json!(pid);
    record["process_start"] = json!(storage::process_start(pid));
    record["subagents"][ID] = json!({"external":true,"provider":"codex","conversation_id":"child",
        "name":"external child","state":"working","started":1.0,"updated":2.0,
        "reply_id":"original-reply"});
    record["subagents"][ID]["original_process"] =
        subagent_lifecycle::capture(&record, child.id()).expect("the child's process identity");
    record["subagents"][ID]["original_process"]["parent_start"] = json!("stale-parent-start");
    subagent_lifecycle::reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["process_state"], "unknown");
    assert_eq!(record["subagents"][ID]["display_state"], "unknown");
    assert_eq!(record["subagents"][ID]["process_unknown"], true);
    assert!(child.try_wait().expect("the child").is_none());
    let before = record["subagents"][ID].clone();
    let event = json!({"hook_event_name":"SubagentStart","session_id":"child",
        "agent_id":"grandchild-1","agent_type":"general-purpose"});
    let scoped = external_agents::route(&mut record, &event).expect("the external child route");
    telemetry::observe(&mut record, &scoped);
    journal::update_activity(&mut record, &scoped);
    if reconciled {
        subagent_lifecycle::reconcile(&mut record);
    }
    let after = record["subagents"][ID].clone();
    assert!(child.try_wait().expect("the child").is_none());
    let listed = row(record);
    child.kill().expect("kill the child");
    child.wait().expect("reap the child");
    (
        listed,
        json!({"before":before,"after":after,"reconciled_after":reconciled}),
    )
}

/// The same live process captured under an old run: a fresh Start must not
/// adopt it for the new run.
fn external_old_run() -> (Value, Value) {
    const ID: &str = "external:codex:22222222-2222-4222-8222-222222222222";
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("a sleeping child");
    let mut record = record("claude");
    let pid = std::process::id();
    record["pid"] = json!(pid);
    record["process_start"] = json!(storage::process_start(pid));
    record["subagents"][ID] = json!({"external":true,"provider":"codex","conversation_id":"child",
        "name":"external child","state":"working","started":1.0,"updated":2.0,
        "reply_id":"original-reply"});
    record["subagents"][ID]["original_process"] =
        subagent_lifecycle::capture(&record, child.id()).expect("the child's process identity");
    record["run_id"] = json!("new-run");
    subagent_lifecycle::reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["process_unknown"], true);
    let before = record["subagents"][ID].clone();
    let event = json!({"hook_event_name":"SubagentStart","session_id":"child",
        "agent_id":"grandchild-1","agent_type":"general-purpose"});
    let scoped = external_agents::route(&mut record, &event).expect("the external child route");
    telemetry::observe(&mut record, &scoped);
    journal::update_activity(&mut record, &scoped);
    subagent_lifecycle::reconcile(&mut record);
    let after = record["subagents"][ID].clone();
    assert_eq!(after["original_process"], before["original_process"]);
    assert_eq!(after["original_process"]["run_id"], "run-claude");
    assert!(child.try_wait().expect("the child").is_none());
    let listed = row(record);
    child.kill().expect("kill the child");
    child.wait().expect("reap the child");
    (
        listed,
        json!({"before":before,"after":after,"original_alive_through_listing":true}),
    )
}

/// An uncertain child settled two ways: the process is re-captured while live
/// and bound (working), or it exits (ended).
fn external_settled(ended: bool) -> (Value, Value) {
    const ID: &str = "external:codex:22222222-2222-4222-8222-222222222222";
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("a sleeping child");
    let mut record = record("claude");
    let pid = std::process::id();
    record["pid"] = json!(pid);
    record["process_start"] = json!(storage::process_start(pid));
    record["subagents"][ID] = json!({"external":true,"provider":"codex","conversation_id":"child",
        "name":"external child","state":"working","started":1.0,"updated":2.0,
        "reply_id":"original-reply"});
    record["subagents"][ID]["original_process"] =
        subagent_lifecycle::capture(&record, child.id()).expect("the child's process identity");
    record["subagents"][ID]["original_process"]["parent_start"] = json!("stale-parent-start");
    subagent_lifecycle::reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["process_unknown"], true);
    if ended {
        child.kill().expect("kill the child");
        child.wait().expect("reap the child");
    } else {
        record["subagents"][ID]["original_process"] =
            subagent_lifecycle::capture(&record, child.id()).expect("the child's process identity");
    }
    subagent_lifecycle::reconcile(&mut record);
    let after = record["subagents"][ID].clone();
    let listed = row(record);
    child.kill().expect("kill the child");
    child.wait().expect("reap the child");
    (listed, json!({"after":after,"ended":ended}))
}

/// An external child interrupted, then its lifecycle uncertainty, then a
/// fresh Start: which record survives.
fn external_interrupt_uncertain() -> (Value, Value) {
    const ID: &str = "external:codex:22222222-2222-4222-8222-222222222222";
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("a sleeping child");
    let mut record = record("claude");
    let pid = std::process::id();
    record["pid"] = json!(pid);
    record["process_start"] = json!(storage::process_start(pid));
    record["subagents"][ID] = json!({"external":true,"provider":"codex","conversation_id":"child",
        "name":"external child","state":"working","started":1.0,"updated":2.0,
        "reply_id":"original-reply"});
    record["subagents"][ID]["original_process"] =
        subagent_lifecycle::capture(&record, child.id()).expect("the child's process identity");
    let interrupt = json!({"hook_event_name":"Interrupt","session_id":"child",
        "agent_id":"grandchild-0"});
    let scoped = external_agents::route(&mut record, &interrupt).expect("the external child route");
    telemetry::observe(&mut record, &scoped);
    journal::update_activity(&mut record, &scoped);
    let interrupted = record["subagents"][ID].clone();
    record["subagents"][ID]["original_process"]["parent_start"] = json!("stale-parent-start");
    subagent_lifecycle::reconcile(&mut record);
    let uncertain = record["subagents"][ID].clone();
    let start = json!({"hook_event_name":"SubagentStart","session_id":"child",
        "agent_id":"grandchild-1","agent_type":"general-purpose"});
    let scoped = external_agents::route(&mut record, &start).expect("the external child route");
    telemetry::observe(&mut record, &scoped);
    journal::update_activity(&mut record, &scoped);
    subagent_lifecycle::reconcile(&mut record);
    let after = record["subagents"][ID].clone();
    let listed = row(record);
    child.kill().expect("kill the child");
    child.wait().expect("reap the child");
    (
        listed,
        json!({"interrupted":interrupted,"uncertain":uncertain,"after_start":after}),
    )
}

/// A legacy external child with no original process, retired, then reconciled
/// again and listed: the explicit end must be settled and visible.
fn external_retired() -> (Value, Value) {
    const ID: &str = "external:codex:22222222-2222-4222-8222-222222222222";
    let mut record = record("claude");
    record["subagents"][ID] = json!({"external":true,"provider":"codex","conversation_id":"child",
        "name":"external child","state":"working","started":1.0,"updated":2.0,
        "reply_id":"original-reply","attention_id":"original-attention"});
    subagent_lifecycle::reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["process_unknown"], true);
    let uncertain = record["subagents"][ID].clone();
    let request: subagent_lifecycle::Retire = serde_json::from_value(json!({
        "request_id":uuid::Uuid::new_v4().to_string(),
        "expected_run_id":string(&record,"run_id"),
        "expected_conversation_id":string(&record,"conversation_id"),
        "agent_id":ID,
        "expected_updated":2.0,
    }))
    .expect("the retirement request");
    subagent_lifecycle::retire_record(&mut record, request).expect("the explicit retirement");
    subagent_lifecycle::reconcile(&mut record);
    let retired = record["subagents"][ID].clone();
    let listed = row(record);
    (listed, json!({"uncertain":uncertain,"retired":retired}))
}

fn produce() -> Value {
    let mut rows = serde_json::Map::new();
    let mut evidence = serde_json::Map::new();
    // Claude: the two orders of one child's events, and its StopFailure end.
    let mut start_stop = record("claude");
    apply(
        &mut start_stop,
        &[
            claude("SubagentStart", "agent-a15"),
            claude("SubagentStop", "agent-a15"),
        ],
    );
    rows.insert("claude_start_stop".into(), row(start_stop));
    let mut stop_start = record("claude");
    apply(&mut stop_start, &[claude("SubagentStop", "agent-a15")]);
    apply(&mut stop_start, &[claude("SubagentStart", "agent-a15")]);
    evidence.insert(
        "claude_stop_start".into(),
        stop_start["subagents"]["agent-a15"].clone(),
    );
    rows.insert("claude_stop_start".into(), row(stop_start));
    let mut late = record("claude");
    apply(&mut late, &[claude("SubagentStop", "agent-a15")]);
    std::thread::sleep(Duration::from_millis(1100));
    apply(&mut late, &[claude("SubagentStart", "agent-a15")]);
    rows.insert("claude_late_start".into(), row(late));
    let mut failed = record("claude");
    apply(&mut failed, &[claude("StopFailure", "agent-a15")]);
    apply(&mut failed, &[claude("SubagentStart", "agent-a15")]);
    evidence.insert(
        "claude_stop_failure_start".into(),
        failed["subagents"]["agent-a15"].clone(),
    );
    rows.insert("claude_stop_failure_start".into(), row(failed));
    let mut working = record("claude");
    apply(&mut working, &[claude("SubagentStart", "agent-a15")]);
    apply(&mut working, &[claude("SubagentStart", "agent-a15")]);
    rows.insert("claude_working_start".into(), row(working));
    // Codex: the same two orders, an Interrupt that is not current work, and
    // a non-Claude StopFailure that writes the same unknown display.
    let mut codex_start_stop = record("codex");
    apply(
        &mut codex_start_stop,
        &[codex("SubagentStart", "c1"), codex("SubagentStop", "c1")],
    );
    rows.insert("codex_start_stop".into(), row(codex_start_stop));
    let mut codex_stop_start = record("codex");
    apply(
        &mut codex_stop_start,
        &[codex("SubagentStop", "c1"), codex("SubagentStart", "c1")],
    );
    rows.insert("codex_stop_start".into(), row(codex_stop_start));
    let mut interrupted = record("codex");
    apply(&mut interrupted, &[codex("Interrupt", "c1")]);
    apply(&mut interrupted, &[codex("SubagentStart", "c1")]);
    evidence.insert(
        "codex_interrupt_start".into(),
        interrupted["subagents"]["c1"].clone(),
    );
    rows.insert("codex_interrupt_start".into(), row(interrupted));
    let mut resumed = record("codex");
    apply(&mut resumed, &[codex("Interrupt", "c1")]);
    apply(
        &mut resumed,
        &[json!({"hook_event_name":"PreToolUse","agent_id":"c1","tool_name":"Bash"})],
    );
    rows.insert("codex_interrupt_tool".into(), row(resumed));
    let mut stop_failed = record("codex");
    apply(&mut stop_failed, &[codex("StopFailure", "c1")]);
    apply(&mut stop_failed, &[codex("SubagentStart", "c1")]);
    evidence.insert(
        "codex_stop_failure_start".into(),
        stop_failed["subagents"]["c1"].clone(),
    );
    rows.insert("codex_stop_failure_start".into(), row(stop_failed));
    // Kimi: profile counts, which carry no child identity.
    let mut kimi_start_stop = record("kimi");
    apply(
        &mut kimi_start_stop,
        &[
            kimi("SubagentStart", "coder"),
            kimi("SubagentStop", "coder"),
        ],
    );
    rows.insert("kimi_start_stop".into(), row(kimi_start_stop));
    let mut kimi_stop_start = record("kimi");
    apply(
        &mut kimi_stop_start,
        &[
            kimi("SubagentStop", "coder"),
            kimi("SubagentStart", "coder"),
        ],
    );
    rows.insert("kimi_stop_start".into(), row(kimi_stop_start));
    // Two children, one of them reordered after its own Stop.
    let mut both = record("claude");
    apply(
        &mut both,
        &[
            claude("SubagentStart", "a"),
            claude("SubagentStart", "b"),
            claude("SubagentStop", "a"),
            claude("SubagentStop", "b"),
            claude("SubagentStart", "b"),
        ],
    );
    rows.insert("two_children_one_reordered".into(), row(both));
    // A child pruned from a record above the cap; its late Start has no
    // ended entry to meet (the honest limit).
    let mut pruned = record("claude");
    let stops: Vec<Value> = (0..66)
        .map(|index| claude("SubagentStop", &format!("agent-p{index:02}")))
        .collect();
    apply(&mut pruned, &stops);
    let held: BTreeSet<String> = pruned["subagents"]
        .as_object()
        .map(|children| children.keys().cloned().collect())
        .unwrap_or_default();
    let missing = (0..66)
        .map(|index| format!("agent-p{index:02}"))
        .find(|id| !held.contains(id))
        .expect("one child was pruned");
    apply(&mut pruned, &[claude("SubagentStart", &missing)]);
    rows.insert("claude_pruned_start".into(), row(pruned));
    // A real new child still counts, and a Stop without a Start keeps its
    // meaning.
    let mut start_only = record("claude");
    apply(&mut start_only, &[claude("SubagentStart", "agent-a15")]);
    rows.insert("claude_start_only".into(), row(start_only));
    let mut stop_only = record("claude");
    apply(&mut stop_only, &[claude("SubagentStop", "agent-a15")]);
    rows.insert("claude_stop_only".into(), row(stop_only));
    // A verified process exit ends the child without any hook; a late Start
    // keeps that end and its provenance.
    let (ended_row, ended_before, ended_after) = external_process_end();
    rows.insert("external_process_end_start".into(), ended_row);
    evidence.insert(
        "external_process_end_start".into(),
        json!({"before":ended_before,"after":ended_after}),
    );
    // A Kimi catalog removal writes `unknown`, not an end; the synthetic Start
    // shows the writer turns it working (no supported hook names a native ID).
    let (removed_row, removed_before, removed_after) = kimi_catalog_removed();
    rows.insert("kimi_removed_start".into(), removed_row);
    evidence.insert(
        "kimi_removed_start".into(),
        json!({"before":removed_before,"after":removed_after}),
    );
    for reconciled in [false, true] {
        let name = if reconciled {
            "external_uncertain_start_reconciled"
        } else {
            "external_uncertain_start"
        };
        let (uncertain_row, uncertain_evidence) = external_process_uncertain(reconciled);
        rows.insert(name.into(), uncertain_row);
        evidence.insert(name.into(), uncertain_evidence);
    }
    let (old_run_row, old_run_evidence) = external_old_run();
    rows.insert("external_old_run_start".into(), old_run_row);
    evidence.insert("external_old_run_start".into(), old_run_evidence);
    for ended in [false, true] {
        let name = if ended {
            "external_uncertain_settled_ended"
        } else {
            "external_uncertain_settled_live"
        };
        let (settled_row, settled_evidence) = external_settled(ended);
        rows.insert(name.into(), settled_row);
        evidence.insert(name.into(), settled_evidence);
    }
    let (retired_row, retired_evidence) = external_retired();
    rows.insert("external_retired".into(), retired_row);
    evidence.insert("external_retired".into(), retired_evidence);
    let (interrupt_row, interrupt_evidence) = external_interrupt_uncertain();
    rows.insert("external_interrupt_uncertain_start".into(), interrupt_row);
    evidence.insert(
        "external_interrupt_uncertain_start".into(),
        interrupt_evidence,
    );
    json!({"rows":rows,"evidence":evidence})
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn reordered_children_child() {
    println!("SUBAGENT-ORDER {}", produce());
}
