//! zerus-63rru.88 live tests: a child's end is final for its ID. A
//! non-start event of an ended child (a delayed PreToolUse or PostToolUse
//! applied after its SubagentStop) is recorded as history only and never
//! sets the child working again; Claude and Codex name every subagent
//! invocation with a fresh ID, so no event of theirs revives an ended
//! child; a Kimi profile child really resumes under its profile name, so
//! only its own start event revives it. Each record starts as the
//! supervisor and its main SessionStart hook build it, the child events go
//! through the production writer (`telemetry::observe` then
//! `journal::update_activity`, the order the hook entry uses), and each
//! row is made by the listing's own steps.
use super::*;
use crate::state::listing_fixture::{confirmed, launched_record, listed_live_row};
use serde_json::{json, Value};

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

fn child(record: &Value, id: &str) -> Value {
    record["subagents"][id].clone()
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

/// The reproduction's live form (zerus-63rru.88): the Stop applied before
/// a delayed PreToolUse or PostToolUse of the same child, for the Claude
/// frame (id plus type) and the Codex frame (id only). The child stays
/// finished, the event is recorded as history, and the listed row reads no
/// active child.
#[test]
fn a_delayed_child_tool_event_after_its_stop_is_recorded_as_history_only() {
    for (agent, builder, kinds) in [
        (
            "claude",
            claude as fn(&str, &str) -> Value,
            &["PreToolUse", "PostToolUse"][..],
        ),
        (
            "codex",
            codex as fn(&str, &str) -> Value,
            &["PreToolUse", "PostToolUse"][..],
        ),
    ] {
        let mut record = record(agent);
        let id = if agent == "claude" { "agent-a1" } else { "d1" };
        apply(
            &mut record,
            &[builder("SubagentStart", id), builder("SubagentStop", id)],
        );
        assert_eq!(child(&record, id)["state"], "finished", "{agent}");
        for kind in kinds {
            apply(&mut record, &[builder(kind, id)]);
            assert_eq!(
                child(&record, id)["state"],
                "finished",
                "{agent} {kind}: the delayed event set the child working again"
            );
            assert_eq!(
                child(&record, id)["tracking_evidence"]["kind"].as_str(),
                Some(*kind),
                "{agent} {kind}: the event is not recorded"
            );
            let row = listed_live_row(record.clone()).expect("a listed row");
            assert_ne!(
                row["subagent_active_count"].as_u64(),
                Some(1),
                "{agent} {kind}: the member reads one active child after a delayed event"
            );
        }
    }
}

/// A Claude child is never revived: its late Start is dropped (the fresh
/// agent_id rule), and a tool event after the end is history only.
#[test]
fn a_claude_child_is_never_revived() {
    let mut record = record("claude");
    apply(
        &mut record,
        &[
            claude("SubagentStart", "agent-a1"),
            claude("SubagentStop", "agent-a1"),
            claude("SubagentStart", "agent-a1"),
            claude("PreToolUse", "agent-a1"),
        ],
    );
    assert_eq!(child(&record, "agent-a1")["state"], "finished");
}

/// A Kimi profile child really resumes under its profile name: a delayed
/// tool event is history only, and the profile's own start event revives
/// it.
#[test]
fn a_kimi_profile_child_resumes_with_its_own_start() {
    let mut record = record("kimi");
    apply(
        &mut record,
        &[
            kimi("SubagentStart", "profile-1"),
            kimi("SubagentStop", "profile-1"),
            kimi("PreToolUse", "profile-1"),
        ],
    );
    assert_eq!(
        child(&record, "profile-1")["state"],
        "finished",
        "a delayed tool event revived the profile child"
    );
    apply(&mut record, &[kimi("SubagentStart", "profile-1")]);
    assert_eq!(
        child(&record, "profile-1")["state"],
        "working",
        "the profile's own start did not revive it"
    );
}

/// zerus-63rru.93: Kimi's frames carry nothing orderable per invocation
/// (the measure in the report), so a racing Start after the child's own
/// Stop revives it, and the next profile-group event settles the revival
/// once it has had no event of its own for the settle period.
fn quiet(h: &mut Value) {
    let at = h["subagents"]["profile-1"]["updated"].as_f64().unwrap();
    h["subagents"]["profile-1"]["updated"] = json!(at - 61.0);
}

#[test]
fn a_kimi_start_after_its_own_stop_is_settled_by_the_next_group_event_in_both_orders() {
    for order in [
        &[
            kimi("SubagentStart", "profile-1"),
            kimi("SubagentStop", "profile-1"),
            kimi("SubagentStart", "profile-1"),
        ][..],
        &[
            kimi("SubagentStop", "profile-1"),
            kimi("SubagentStart", "profile-1"),
        ][..],
    ] {
        let mut record = record("kimi");
        apply(&mut record, order);
        assert_eq!(
            child(&record, "profile-1")["state"], "working",
            "the revived child works after its own stop"
        );
        quiet(&mut record);
        // The next profile-group event settles the revival; the other
        // profile's own short invocation then ends, leaving the member free.
        apply(&mut record, &[kimi("SubagentStart", "profile-2")]);
        let ended = child(&record, "profile-1");
        assert_eq!(ended["state"], "ended", "{ended}");
        apply(&mut record, &[kimi("SubagentStop", "profile-2")]);
        let row = listed_live_row(record.clone()).expect("a listed row");
        assert_eq!(
            row["subagent_active_count"], 0,
            "the member is not free: {row}"
        );
    }
}

#[test]
fn a_genuine_kimi_resume_after_a_long_gap_stays_working() {
    let mut record = record("kimi");
    apply(
        &mut record,
        &[kimi("SubagentStart", "profile-1"), kimi("SubagentStop", "profile-1")],
    );
    assert_eq!(child(&record, "profile-1")["state"], "finished");
    // A genuine resume after a long gap: the Start revives the child.
    quiet(&mut record);
    apply(&mut record, &[kimi("SubagentStart", "profile-1")]);
    assert_eq!(child(&record, "profile-1")["state"], "working");
    // Its own tool event within the settle period keeps it working, and the
    // next group event settles nothing.
    apply(
        &mut record,
        &[json!({"hook_event_name":"PreToolUse","agent_id":"main",
            "agent_name":"profile-1","tool_name":"Tool"})],
    );
    apply(&mut record, &[kimi("SubagentStop", "profile-2")]);
    assert_eq!(child(&record, "profile-1")["state"], "working");
}

#[test]
fn a_settled_revival_frees_the_member_in_the_teams_fleet() {
    let mut record = record("kimi");
    apply(
        &mut record,
        &[kimi("SubagentStart", "profile-1"), kimi("SubagentStop", "profile-1")],
    );
    quiet(&mut record);
    apply(&mut record, &[kimi("SubagentStart", "profile-1")]);
    quiet(&mut record);
    apply(&mut record, &[kimi("SubagentStart", "profile-2")]);
    apply(&mut record, &[kimi("SubagentStop", "profile-2")]);
    // The fleet's own free read over the listed row: the settled child
    // leaves no working child and the member is free again.
    let row = listed_live_row(record.clone()).expect("a listed row");
    assert_eq!(row["subagent_active_count"], 0, "the member is not free: {row}");
}

/// Round 2: the settle needs no later event at all — the read-time rule in
/// the listing's count producer shows the member free after the period, and
/// the main's own next event persists the end.
#[test]
fn the_race_followed_by_no_further_event_reads_free_after_the_period() {
    let mut record = record("kimi");
    apply(
        &mut record,
        &[
            kimi("SubagentStart", "profile-1"),
            kimi("SubagentStop", "profile-1"),
            kimi("SubagentStart", "profile-1"),
        ],
    );
    assert_eq!(child(&record, "profile-1")["state"], "working");
    quiet(&mut record);
    // No further event at all: the listing the fleet reads shows no active
    // child and the member reads free, with no write.
    let row = listed_live_row(record.clone()).expect("a listed row");
    assert_eq!(
        row["subagent_active_count"], 0,
        "the revived child kept the member busy: {row}"
    );
    // The settled and work-remains reads agree: no idle-only action waits
    // for an event that will never come.
    assert!(
        !crate::state::telemetry::work_remains(&record),
        "work remains after the period: {record}"
    );
    assert!(
        crate::state::telemetry::settled(&record),
        "the session is not settled after the period: {record}"
    );
    // The persisted row is untouched by the read: only the read counts it.
    assert_eq!(child(&record, "profile-1")["state"], "working");
}

#[test]
fn the_main_agents_turn_end_persists_the_settle() {
    let mut record = record("kimi");
    apply(
        &mut record,
        &[
            kimi("SubagentStart", "profile-1"),
            kimi("SubagentStop", "profile-1"),
            kimi("SubagentStart", "profile-1"),
        ],
    );
    quiet(&mut record);
    // The main's own Stop (no subagent event) persists the end.
    apply(
        &mut record,
        &[json!({"hook_event_name":"Stop","agent_id":"main"})],
    );
    let ended = child(&record, "profile-1");
    assert_eq!(ended["state"], "ended", "{ended}");
    // The activity decision ran with the read-time rule, so the persisted
    // activity and phase read idle once the child is settled.
    assert_eq!(record["activity"], "idle", "{record}");
    assert_eq!(record["phase"], "idle", "{record}");
    let row = listed_live_row(record.clone()).expect("a listed row");
    assert_eq!(row["subagent_active_count"], 0, "{row}");
}

/// Round 4: the main's Stop while the child is not yet quiet, then the
/// period passes with no event: the settled read is true (the main's turn
/// has ended, so the busy activity comes from children alone).
#[test]
fn the_stop_before_the_quiet_period_still_reads_settled_after_it() {
    let mut record = record("kimi");
    apply(
        &mut record,
        &[
            kimi("SubagentStart", "profile-1"),
            kimi("SubagentStop", "profile-1"),
            kimi("SubagentStart", "profile-1"),
        ],
    );
    apply(
        &mut record,
        &[json!({"hook_event_name":"Stop","agent_id":"main"})],
    );
    quiet(&mut record);
    assert!(
        crate::state::telemetry::settled(&record),
        "the ended turn is not settled: {record}"
    );
    assert!(!crate::state::telemetry::work_remains(&record));
}

/// Round 3: the persisted settle and the read-time rule agree — a quiet
/// revived profile's whole group zeroes, whatever its active count was.
#[test]
fn the_persisted_settle_zeroes_the_whole_profile_group() {
    let mut record = record("kimi");
    apply(
        &mut record,
        &[
            kimi("SubagentStart", "profile-1"),
            kimi("SubagentStart", "profile-1"),
            kimi("SubagentStop", "profile-1"),
            kimi("SubagentStart", "profile-1"),
        ],
    );
    assert_eq!(child(&record, "profile-1")["state"], "working");
    quiet(&mut record);
    apply(
        &mut record,
        &[json!({"hook_event_name":"Stop","agent_id":"main"})],
    );
    let group = &record["subagent_groups"]["profile-1"];
    assert_eq!(group["active_count"], 0, "{group}");
    assert_eq!(group["completed_count"], 3, "{group}");
    assert_eq!(child(&record, "profile-1")["state"], "ended");
}
