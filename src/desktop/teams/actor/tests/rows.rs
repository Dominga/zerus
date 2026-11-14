//! Fleet rows for the actor's tests, made by the product (zerus-63rru.51).
//!
//! `fixtures/fleet-rows.json` holds one idle live row per agent kind as the
//! listing writes it: Claude, Codex and Kimi through `watch_source::rows`'
//! own steps, DeepSeek through `native_rows`' own steps from a host's `list`
//! answer. Its paths are normalized to `/fixture`. Every seeded fleet row
//! starts from one of these and changes named keys only; a key the producer's
//! row does not have cannot be added. `the_fixture_rows_are_the_producers`
//! makes the rows again in a child test process with a private state root and
//! fails when the fixture and the producer have different keys.
use serde_json::{json, Value};

pub(in crate::desktop::teams) const KINDS: [&str; 4] = ["claude", "codex", "kimi", "dsh"];

const CHILD: &str = "desktop::teams::actor::tests::rows::";

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/fleet-rows.json")).unwrap()
}

/// The producer's idle live row of `kind`.
pub(in crate::desktop::teams) fn producer_row(kind: &str) -> Value {
    let row = fixture()["rows"][kind].clone();
    assert!(row.is_object(), "no producer row for {kind}");
    row
}

/// The producer's row of `kind` with each `(path, value)` set. A path is
/// dotted (`process_work.count`) and must already exist in the producer's
/// row: a seed cannot carry a key the product never writes.
pub(in crate::desktop::teams) fn seed_row(kind: &str, changes: &[(&str, Value)]) -> Value {
    let mut row = producer_row(kind);
    for (path, value) in changes {
        let mut slot = &mut row;
        for part in path.split('.') {
            assert!(
                slot.get(part).is_some(),
                "a seeded {kind} row would carry `{path}`, which the producer's row does not have"
            );
            slot = &mut slot[part];
        }
        *slot = value.clone();
    }
    row
}

/// Every key path of `value`, dotted, with the objects' own paths.
pub(in crate::desktop::teams) fn paths(value: &Value) -> std::collections::BTreeSet<String> {
    fn walk(value: &Value, prefix: &str, out: &mut std::collections::BTreeSet<String>) {
        if let Some(map) = value.as_object() {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                out.insert(path.clone());
                // Maps keyed by an identity (children, read marks) are data.
                if !matches!(key.as_str(), "subagents" | "action_reasons") {
                    walk(child, &path, out);
                }
            }
        }
    }
    let mut out = std::collections::BTreeSet::new();
    walk(value, "", &mut out);
    out
}

/// The keys a DeepSeek host's `list` answer and its rows may carry here,
/// pinned in one place (zerus-63rru.61). A list row carries the keys the
/// contract tests of `src/state/dsh.rs` build list rows with (its
/// `child_summaries` test and its `"list"` stubs), and `projections`, which
/// the bridge's list handler passes through from the host and reads itself
/// (`src/dsh/bridge.mjs`, case 'list') and which alone names a session's
/// children (`projections.values.subagentCatalog`, read by `dsh.rs`). It
/// never carries `jobs` or `shellCalls`: only `inspect` answers with them.
///
/// The bridge's own answer is captured and guarded by `test_dsh_bridge` in
/// `tests/fixtures/dsh-bridge-list-row.json`. Use its idle session here;
/// subagent cases extend that same producer row with the pinned catalog keys.
pub(in crate::desktop::teams) const DEEPSEEK_LIST_KEYS: [&str; 2] = ["generation", "items"];
pub(in crate::desktop::teams) const DEEPSEEK_ROW_KEYS: [&str; 13] = [
    "sessionId",
    "parentSessionId",
    "origin",
    "running",
    "agentAvailable",
    "projections",
    "accountStatus",
    "authRequired",
    "goal",
    "listJobs",
    "nativeRecovery",
    "pendingQuestions",
    "selection",
];
pub(in crate::desktop::teams) const DEEPSEEK_CATALOG_KEYS: [&str; 3] = ["id", "label", "mode"];

/// The captured bridge's idle, available session with this test's identity.
pub(in crate::desktop::teams) fn deepseek_list(conversation: &str) -> Value {
    let capture: Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/dsh-bridge-list-row.json"
    )).unwrap();
    let mut row = capture["list"]["items"].as_array().unwrap().iter()
        .find(|row| row["sessionId"] == "session-idle").unwrap().clone();
    row["sessionId"] = json!(conversation);
    json!({"generation":capture["list"]["generation"],"items":[row]})
}

/// Every key of a list answer is a pinned one.
pub(in crate::desktop::teams) fn assert_pinned_list(list: &Value) {
    let keys = |v: &Value| -> Vec<String> {
        v.as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    };
    for key in keys(list) {
        assert!(DEEPSEEK_LIST_KEYS.contains(&key.as_str()), "list key {key}");
    }
    for row in list["items"].as_array().unwrap() {
        for key in keys(row) {
            assert!(
                DEEPSEEK_ROW_KEYS.contains(&key.as_str()),
                "list row key {key}"
            );
        }
        if let Some(values) = row.get("projections") {
            assert_eq!(keys(values), ["values"], "projections");
            for key in keys(&values["values"]) {
                assert_eq!(key, "subagentCatalog", "projections.values key");
            }
            for child in values["values"]["subagentCatalog"]
                .as_array()
                .into_iter()
                .flatten()
            {
                for key in keys(child) {
                    assert!(
                        DEEPSEEK_CATALOG_KEYS.contains(&key.as_str()),
                        "catalog key {key}"
                    );
                }
            }
        }
    }
}

/// One idle live session of `kind`, as its saved binding records it.
pub(in crate::desktop::teams) fn record(kind: &str, root: &str) -> Value {
    let name = format!("{kind}/example/member");
    if kind == "dsh" {
        return json!({"backend":"dsh","name":name,"agent":"dsh","agent_home":format!("{root}/.dsh"),
            "cwd":"/","conversation_id":"session-00000000-0000-4000-8000-0000000000d5",
            "run_id":"run-dsh","created":1793000000.0,"launch_id":"00000000-0000-4000-8000-0000000000d4",
            "paused":false,"session_id":"00000000-0000-4000-8000-0000000000d6"});
    }
    json!({"version":1,"run_identity_version":1,"name":name,"agent":kind,
        "run_id":format!("run-{kind}"),"conversation_id":format!("conversation-{kind}"),
        "session_id":"00000000-0000-4000-8000-000000000051","launch_id":"00000000-0000-4000-8000-000000000050",
        "cwd":"/","launch_dir":"/","created":1793000000,
        "activity":"idle","phase":"idle","conversation_state":"active","last_event_at":1793000001.0})
}

/// Makes the four rows in this process, whose state root is private.
fn produce() -> Value {
    let root = std::env::var("HOME").unwrap();
    let mut rows = serde_json::Map::new();
    for kind in KINDS {
        let record = record(kind, &root);
        let row = if kind == "dsh" {
            let list = deepseek_list(record["conversation_id"].as_str().unwrap());
            crate::state::listing_fixture::listed_deepseek_row(&record, &list)
        } else {
            crate::state::listing_fixture::listed_live_row(record).unwrap()
        };
        rows.insert(kind.into(), row);
    }
    // Paths of this process's private root are not the producer's choice.
    let text = Value::Object(rows).to_string().replace(&root, "/fixture");
    json!({"about":"Idle live fleet rows made by the listing's own steps; regenerate with the ignored test regenerate_fleet_rows.","rows":serde_json::from_str::<Value>(&text).unwrap()})
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn producer_rows_child() {
    println!("FLEET-ROWS {}", produce());
}

/// The rows the producer makes now, from a child with a private root.
pub(in crate::desktop::teams) fn produced() -> Value {
    let text = super::real_rows::isolated(&format!("{CHILD}producer_rows_child"));
    let line = text
        .lines()
        .find_map(|line| line.split_once("FLEET-ROWS ").map(|(_, rows)| rows))
        .expect("producer rows");
    serde_json::from_str(line).unwrap()
}

#[test]
fn the_fixture_rows_are_the_producers() {
    let fresh = produced();
    for kind in KINDS {
        let made = paths(&fresh["rows"][kind]);
        let kept = paths(&producer_row(kind));
        let extra: Vec<_> = kept.difference(&made).collect();
        let missing: Vec<_> = made.difference(&kept).collect();
        assert!(
            extra.is_empty() && missing.is_empty(),
            "{kind}: the fixture carries {extra:?} that the producer does not write and lacks {missing:?}; regenerate it"
        );
    }
}

#[test]
#[ignore = "writes src/desktop/teams/actor/tests/fixtures/fleet-rows.json"]
fn regenerate_fleet_rows() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/desktop/teams/actor/tests/fixtures/fleet-rows.json");
    std::fs::write(
        path,
        serde_json::to_string_pretty(&produced()).unwrap() + "\n",
    )
    .unwrap();
}

#[test]
fn a_seed_cannot_carry_a_key_the_producer_does_not_write() {
    for (kind, path) in [
        ("claude", "subagents_active"),
        ("codex", "state"),
        ("kimi", "process_work.total"),
    ] {
        let refused = std::panic::catch_unwind(|| seed_row(kind, &[(path, json!(0))]));
        assert!(refused.is_err(), "{kind} {path}");
    }
    assert_eq!(
        seed_row("dsh", &[("state", json!("paused"))])["state"],
        "paused"
    );
}

/// Every key the actor's reader reads exists in a producer's row; one that
/// must be present for a row to count is present in every kind's row.
#[test]
fn every_key_the_reader_reads_is_written_by_a_producer() {
    let rows: Vec<_> = KINDS
        .iter()
        .map(|kind| (kind, paths(&producer_row(kind))))
        .collect();
    for (path, required) in super::super::fleet::READ {
        let having: Vec<_> = rows
            .iter()
            .filter(|(_, p)| p.contains(path))
            .map(|(k, _)| **k)
            .collect();
        if required {
            assert_eq!(
                having.len(),
                KINDS.len(),
                "`{path}` is read as required but only {having:?} write it"
            );
        } else {
            assert!(
                !having.is_empty(),
                "`{path}` is read but no producer row has it"
            );
        }
    }
}

/// The reader's key list names every key it reads: removing any key of a
/// producer's row that changes what the reader answers must be listed.
#[test]
fn the_readers_key_list_names_every_key_it_reads() {
    use super::super::fleet::{Facts, Identity, READ};
    fn remove(row: &mut Value, path: &str) {
        let mut parts: Vec<&str> = path.split('.').collect();
        let last = parts.pop().unwrap();
        let mut slot = row;
        for part in parts {
            slot = &mut slot[part];
        }
        if let Some(map) = slot.as_object_mut() {
            map.remove(last);
        }
    }
    let answer = |row: &Value| {
        let mut facts = Facts::of(row);
        // The shared presentation's status reads its own keys (client core).
        facts.ready = true;
        let named = Identity::of(row);
        format!("{facts:?} {named:?}")
    };
    let listed = |path: &str| {
        READ.iter()
            .any(|(read, _)| *read == path || read.starts_with(&format!("{path}.")))
    };
    assert!(!READ.is_empty());
    for kind in KINDS {
        let row = producer_row(kind);
        let before = answer(&row);
        for path in paths(&row) {
            let mut changed = row.clone();
            remove(&mut changed, &path);
            if answer(&changed) != before {
                assert!(
                    listed(&path),
                    "{kind}: the reader reads `{path}`, which its key list does not name"
                );
            }
        }
    }
}

/// The fixture's DeepSeek list answer carries only pinned keys: none the
/// bridge's list handler does not pass (zerus-63rru.61).
#[test]
fn the_deepseek_list_answer_carries_only_the_pinned_keys() {
    assert_pinned_list(&deepseek_list(
        "session-00000000-0000-4000-8000-0000000000d5",
    ));
}

/// A quiet revived Kimi profile child, as the hooks record it.
fn quiet_kimi_record() -> Value {
    let mut record = record("kimi", "/fixture");
    record["subagents"] = json!({"profile-1": {
        "name":"profile-1","started":1793000000.0,"state":"working",
        "revived_at":1793000060.0,"updated":1792999940.0}});
    record["subagent_groups"] = json!({"profile-1": {
        "label":"profile-1","active_count":1,"completed_count":1,"total_count":2,
        "updated":1792999940.0}});
    record["subagent_groups_complete"] = json!(true);
    record
}

/// The rows the settled reads below need, made by the listing's own steps in
/// a child test process with a private state root (zerus-63rru.117). The
/// listing reads a live terminal's pane paths through `tmux`, whose client is
/// a direct child of the process that lists; `processes::control::roots`
/// counts a direct child shell as one of the row's own background jobs. Made
/// in the shared test process, a row reads a sibling test's client as its own
/// background job under load, so one test at a time in the child keeps a
/// row's own process clear of another test's children. The rows are still the
/// producer's: `listing_fixture::listed_live_row` runs its steps here.
fn settled_rows() -> Value {
    static ROWS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    ROWS.get_or_init(|| {
        let text = super::real_rows::isolated(&format!("{CHILD}settled_rows_child"));
        let rows = text
            .lines()
            .find_map(|line| line.split_once("SETTLED-ROWS "))
            .map(|(_, rows)| rows)
            .expect("settled rows");
        serde_json::from_str::<Value>(rows).unwrap()
    })
    .clone()
}

/// Makes `settled_rows`' rows; run by its isolated wrapper.
#[test]
#[ignore = "run by its isolated wrapper"]
fn settled_rows_child() {
    let rows = json!({
        "quiet_kimi": crate::state::listing_fixture::listed_live_row(quiet_kimi_record()).unwrap(),
        "mixed_complete":
            crate::state::listing_fixture::listed_live_row(mixed_record(true)).unwrap(),
        "mixed_incomplete":
            crate::state::listing_fixture::listed_live_row(mixed_record(false)).unwrap(),
        "claude":
            crate::state::listing_fixture::listed_live_row(record("claude", "/fixture")).unwrap(),
    });
    println!("SETTLED-ROWS {rows}");
}

/// zerus-63rru.93 round 3: the fleet's own free read over the real listed
/// row — a quiet revived Kimi profile child leaves no active child and no
/// unsettled count, so the member reads free.
#[test]
fn a_quiet_revived_kimi_child_reads_free_in_the_fleet() {
    let row = settled_rows()["quiet_kimi"].clone();
    assert_eq!(row["subagent_active_count"], 0, "{row}");
    let facts = super::super::fleet::Facts::of(&row);
    assert!(facts.free(), "{facts:?}");
}

/// zerus-63rru.68: the mixed-hooks row's two facts apart — the groups'
/// completeness has its own field, so an unknown external child does not
/// keep a member with complete groups unsettled.
fn mixed_record(groups_complete: bool) -> Value {
    let mut record = record("kimi", "/fixture");
    record["subagents"] = json!({
        "profile-1": {
            "name":"profile-1","started":1793000000.0,"state":"finished",
            "updated":1793000000.0
        },
        "ext-1": {
            "name":"ext","started":1793000000.0,"state":"ended",
            "display_state":"unknown","external":true,"updated":1793000000.0
        },
    });
    record["subagent_groups"] = json!({"profile-1": {
        "label":"profile-1","active_count":0,"completed_count":1,"total_count":1,
        "updated":1793000000.0
    }});
    record["subagent_groups_complete"] = json!(groups_complete);
    record
}

#[test]
fn a_mixed_row_with_an_unknown_external_child_and_complete_groups_reads_settled() {
    let row = settled_rows()["mixed_complete"].clone();
    assert_eq!(row["subagent_source"], "mixed_hooks", "{row}");
    assert_eq!(row["subagent_counts_complete"], false, "{row}");
    assert_eq!(row["subagent_groups_complete"], true, "{row}");
    let facts = super::super::fleet::Facts::of(&row);
    assert!(facts.free(), "{facts:?}");
}

#[test]
fn incomplete_groups_stay_unsettled() {
    let row = settled_rows()["mixed_incomplete"].clone();
    assert_eq!(row["subagent_source"], "mixed_hooks", "{row}");
    assert_eq!(row["subagent_groups_complete"], false, "{row}");
    let facts = super::super::fleet::Facts::of(&row);
    assert!(!facts.free(), "{facts:?}");
}

#[test]
fn an_older_row_without_the_group_field_reads_as_today() {
    let mut row = settled_rows()["mixed_complete"].clone();
    row.as_object_mut().unwrap().remove("subagent_groups_complete");
    // The fallback reads the old combined flag, which is false for the
    // unknown external child: unsettled, exactly as before the split.
    let facts = super::super::fleet::Facts::of(&row);
    assert!(!facts.free(), "{facts:?}");
}

/// zerus-63rru.68 round 2: the independent reviewer's two real-writer probes,
/// committed: History never keeps a member, and the legacy combined flag
/// keeps its exact old meaning for the unchanged readers.
fn review68_produced() -> Value {
    let text = super::real_rows::isolated("state::subagent_order::reordered_children_child");
    let row = text
        .lines()
        .find_map(|line| line.split_once("SUBAGENT-ORDER ").map(|(_, value)| value))
        .unwrap();
    serde_json::from_str(row).unwrap()
}

#[test]
fn required_history_rule() {
    let rows = review68_produced();
    for name in ["codex_interrupt_start", "external_process_end_start"] {
        let row = &rows["rows"][name];
        let facts = super::super::fleet::Facts::of(row);
        assert!(
            facts.free(),
            "the plan says History never keeps a member: {name} {facts:?}"
        );
        assert_eq!(row["subagent_active_count"], 0, "{name}");
        assert_eq!(row["subagent_process_unknown_count"], 0, "{name}");
    }
}

#[test]
fn required_additive_legacy_flag() {
    let rows = review68_produced();
    let row = &rows["rows"]["kimi_stop_start"];
    assert_eq!(row["subagent_groups_complete"], false);
    assert_eq!(
        row["subagent_counts_complete"], false,
        "an additive field must not make incomplete groups complete for unchanged readers"
    );
    assert!(
        !zerus_client_core::view::row(row).subagent_counts_complete,
        "the client core reads the old combined flag"
    );

}

/// Round 2's three surviving mutants, each now killed by a committed test.
#[test]
fn a_malformed_groups_field_reads_unsettled() {
    let mut row = settled_rows()["mixed_complete"].clone();
    row["subagent_groups_complete"] = json!("true");
    let facts = super::super::fleet::Facts::of(&row);
    assert!(!facts.free(), "a non-boolean completeness read settled: {facts:?}");
}

#[test]
fn an_unbound_process_child_still_unsettles_history() {
    let mut row = settled_rows()["claude"].clone();
    row["subagent_process_unknown_count"] = json!(1);
    let facts = super::super::fleet::Facts::of(&row);
    assert!(!facts.free(), "the process_unknown guard was dropped: {facts:?}");
    assert_eq!(
        facts.unknown(),
        Some(super::super::ports::Fact::ChildrenUnsettled),
        "{facts:?}"
    );
}

#[test]
fn a_mixed_row_counts_its_active_profile_groups() {
    let mut record = mixed_record(true);
    record["subagent_groups"] = json!({
        "profile-1": {
            "label":"profile-1","active_count":2,"completed_count":1,"total_count":3,
            "updated":1793000000.0
        }
    });
    let row = crate::state::listing_fixture::listed_live_row(record).unwrap();
    assert_eq!(row["subagent_source"], "mixed_hooks", "{row}");
    assert_eq!(
        row["subagent_active_count"], 2,
        "the mixed row dropped its active groups: {row}"
    );
}
