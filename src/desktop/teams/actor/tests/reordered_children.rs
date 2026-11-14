//! A child's Start and Stop are written by two hook processes and can arrive
//! in either order (zerus-63rru.78). The rows are made by the product: the
//! supervisor and SessionStart hook build the record, the hook writer applies
//! the child events, and the listing's own steps make the row this test reads
//! with the actor's facts. A Start for a child the record already holds as
//! ended must not count as active again; the cases the writer cannot tell
//! apart are pinned as they are, with what bounds them.
use super::super::fleet::Facts;
use super::super::ports::Fact;
use serde_json::Value;

const CHILD: &str = "state::subagent_order::reordered_children_child";

fn produced() -> Value {
    let text = super::real_rows::isolated(CHILD);
    let line = text
        .lines()
        .find_map(|line| line.split_once("SUBAGENT-ORDER ").map(|(_, rows)| rows))
        .expect("the child printed its rows");
    serde_json::from_str(line).expect("the rows are one JSON object")
}

/// (row, source, complete flag, count, unsettled, free, unknown fact, seen working)
#[allow(clippy::type_complexity)]
const EXPECTED: [(
    &str,
    &str,
    bool,
    Option<u64>,
    bool,
    bool,
    Option<Fact>,
    Option<bool>,
); 25] = [
    // Claude Code: both orders of one child's events leave it ended.
    (
        "claude_start_stop",
        "hooks",
        true,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    (
        "claude_stop_start",
        "hooks",
        true,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    (
        "claude_late_start",
        "hooks",
        true,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    // A Claude StopFailure ends the child as an error; its late Start keeps it.
    (
        "claude_stop_failure_start",
        "hooks",
        true,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    // A child already working is untouched by another Start.
    (
        "claude_working_start",
        "hooks",
        true,
        Some(1),
        false,
        false,
        None,
        None,
    ),
    // Codex: its events name only the child's ID; the same holds.
    (
        "codex_start_stop",
        "hooks",
        true,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    (
        "codex_stop_start",
        "hooks",
        true,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    // An Interrupt leaves a working child whose display is unknown, which is
    // history; its late Start keeps that, while another event resumes it.
    (
        "codex_interrupt_start",
        "hooks",
        false,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    (
        "codex_interrupt_tool",
        "hooks",
        true,
        Some(1),
        false,
        false,
        None,
        None,
    ),
    (
        "codex_stop_failure_start",
        "hooks",
        false,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    // Kimi's profile hooks carry no child identity: a reordered Start cannot
    // be told from a new child of the same profile, so it stays counted and
    // the count stays incomplete (busy, never silently free).
    (
        "kimi_start_stop",
        "hook_profiles",
        true,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    (
        "kimi_stop_start",
        "hook_profiles",
        false,
        Some(1),
        true,
        false,
        None,
        // A busy row has no process sample, so presence itself is unknown.
        None,
    ),
    // One of two children reordered: the other one's history is unaffected.
    (
        "two_children_one_reordered",
        "hooks",
        true,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    // A verified process exit ends the child with no hook; its late Start
    // keeps that end and its provenance (an unknown count is history here).
    (
        "external_process_end_start",
        "hooks",
        false,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    // A live external child whose parent binding went stale is not an end:
    // the fresh Start resumes it, with or without the hook entry's reconcile.
    (
        "external_uncertain_start",
        "hooks",
        true,
        Some(1),
        false,
        false,
        None,
        None,
    ),
    (
        "external_uncertain_start_reconciled",
        "hooks",
        false,
        Some(0),
        true,
        false,
        Some(Fact::ChildrenUnsettled),
        None,
    ),
    // The same process under an old run is never adopted, and the member is
    // not free while that uncertainty stands.
    (
        "external_old_run_start",
        "hooks",
        false,
        Some(0),
        true,
        false,
        Some(Fact::ChildrenUnsettled),
        None,
    ),
    // An explicit retirement clears the process projection: the ended child
    // counts zero and the member is free again.
    (
        "external_retired",
        "hooks",
        false,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    // A later reconcile settles it: live and bound counts working; gone ends.
    (
        "external_uncertain_settled_live",
        "hooks",
        true,
        Some(1),
        false,
        false,
        None,
        None,
    ),
    (
        "external_uncertain_settled_ended",
        "hooks",
        false,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    // An Interrupt, then the lifecycle's uncertainty, then a Start: the hook's
    // interrupted evidence is the last end and the Start stays out.
    (
        "external_interrupt_uncertain_start",
        "hooks",
        false,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
    // The Kimi catalog's removal writes `unknown`, not an end; a raw Start
    // turns it working (no supported Kimi hook names a native ID).
    (
        "kimi_removed_start",
        "kimi_native",
        true,
        Some(1),
        false,
        false,
        None,
        None,
    ),
    // A Start for a child pruned above the cap has no ended entry to meet.
    (
        "claude_pruned_start",
        "hooks",
        true,
        Some(1),
        false,
        false,
        None,
        None,
    ),
    // Nothing else changes: a real new child counts, a lone Stop stays history.
    (
        "claude_start_only",
        "hooks",
        true,
        Some(1),
        false,
        false,
        None,
        None,
    ),
    (
        "claude_stop_only",
        "hooks",
        true,
        Some(0),
        false,
        true,
        None,
        Some(false),
    ),
];

#[test]
fn a_start_for_an_ended_child_never_counts_as_active_again() {
    let produced = produced();
    let rows = &produced["rows"];
    let evidence = &produced["evidence"];
    for (name, source, complete, count, unsettled, free, unknown, seen) in EXPECTED {
        let row = &rows[name];
        assert_eq!(row["subagent_source"], source, "{name}");
        assert_eq!(row["subagent_counts_complete"], complete, "{name}");
        let facts = Facts::of(row);
        assert_eq!(facts.children, count, "{name}: {row}");
        assert_eq!(facts.children_unsettled, unsettled, "{name}");
        assert_eq!(facts.free(), free, "{name}: {row}");
        assert_eq!(facts.unknown(), unknown, "{name}");
        assert_eq!(facts.seen().map(|seen| seen.working), seen, "{name}");
    }
    // The late announcement is written back, and an end keeps its display and
    // its provenance.
    assert_eq!(evidence["claude_stop_start"]["announced"], true);
    assert_eq!(evidence["claude_stop_start"]["state"], "finished");
    assert_eq!(evidence["claude_stop_failure_start"]["state"], "finished");
    assert_eq!(
        evidence["claude_stop_failure_start"]["display_state"],
        "error"
    );
    assert_eq!(evidence["codex_interrupt_start"]["state"], "working");
    assert_eq!(
        evidence["codex_interrupt_start"]["display_state"],
        "unknown"
    );
    assert_eq!(
        evidence["codex_stop_failure_start"]["display_state"],
        "unknown"
    );
    assert_eq!(
        evidence["external_process_end_start"]["before"]["state"],
        "ended"
    );
    assert_eq!(
        evidence["external_process_end_start"]["after"]["state"],
        "ended"
    );
    assert_eq!(
        evidence["external_process_end_start"]["after"]["completion_source"],
        "original_process_exit"
    );
    assert_eq!(evidence["kimi_removed_start"]["before"]["state"], "unknown");
    assert_eq!(evidence["kimi_removed_start"]["after"]["state"], "working");
    // Process uncertainty is not an end: the fresh Start is not suppressed,
    // but the reconciled uncertainty stands and no count is reported.
    let uncertain = &evidence["external_uncertain_start_reconciled"];
    assert_eq!(uncertain["before"]["state"], "working");
    assert_eq!(uncertain["before"]["display_state"], "unknown");
    assert_eq!(uncertain["before"]["process_unknown"], true);
    assert_eq!(uncertain["after"]["state"], "working");
    assert_eq!(uncertain["after"]["display_state"], "unknown");
    assert_eq!(uncertain["after"]["process_unknown"], true);
    assert_eq!(uncertain["after"]["process_state"], "unknown");
    assert_eq!(
        rows["external_uncertain_start_reconciled"]["subagent_process_unknown_count"],
        1
    );
    assert_eq!(
        rows["external_old_run_start"]["subagent_process_unknown_count"],
        1
    );
    // The old run's process is never adopted for the new run.
    assert_eq!(
        evidence["external_old_run_start"]["after"]["original_process"]["run_id"],
        "run-claude"
    );
    assert_eq!(
        evidence["external_old_run_start"]["after"]["original_process"],
        evidence["external_old_run_start"]["before"]["original_process"]
    );
    // A settled reconcile resumes or ends the child.
    assert_eq!(
        evidence["external_uncertain_settled_live"]["after"]["state"],
        "working"
    );
    assert_eq!(
        evidence["external_uncertain_settled_live"]["after"]["display_state"],
        Value::Null
    );
    assert_eq!(
        evidence["external_uncertain_settled_live"]["after"]["process_unknown"],
        false
    );
    assert_eq!(
        evidence["external_uncertain_settled_ended"]["after"]["state"],
        "ended"
    );
    assert_eq!(
        evidence["external_uncertain_settled_ended"]["after"]["completion_source"],
        "original_process_exit"
    );
    // Retirement clears the process projection and settles the ended child.
    let retired = &evidence["external_retired"];
    assert_eq!(retired["uncertain"]["process_unknown"], true);
    assert_eq!(retired["retired"]["state"], "ended");
    assert_eq!(retired["retired"]["display_state"], "ended");
    assert_eq!(
        retired["retired"]["completion_source"],
        "explicit_retirement"
    );
    assert!(retired["retired"].get("process_unknown").is_none());
    assert!(retired["retired"].get("process_display_state").is_none());
    assert_eq!(
        rows["external_retired"]["subagent_process_unknown_count"],
        0
    );
    // The desktop core reads the same numeric count as before the separate
    // Teams field: unknown work is not current work for reply attention.
    let core = zerus_client_core::view::row(&rows["external_old_run_start"]);
    assert_eq!(core.notify.subagents_active, 0);
    assert_eq!(core.status, "ready");
    // The earlier Interrupt wins: the lifecycle's uncertainty does not replace
    // its evidence, and the later Start stays history.
    let interrupt = &evidence["external_interrupt_uncertain_start"];
    assert_eq!(
        interrupt["interrupted"]["tracking_evidence"]["kind"],
        "Interrupt"
    );
    assert_eq!(interrupt["uncertain"]["process_state"], "unknown");
    assert_eq!(interrupt["after_start"]["display_state"], "unknown");
    assert_eq!(
        interrupt["after_start"]["tracking_evidence"]["kind"],
        "Interrupt"
    );
    assert_eq!(interrupt["after_start"]["process_state"], "unknown");
    // The two orders of one child are indistinguishable to the member: both
    // leave it free, which is what hands it its next task.
    assert_eq!(
        Facts::of(&rows["claude_stop_start"]).free(),
        Facts::of(&rows["claude_start_stop"]).free(),
    );
}
