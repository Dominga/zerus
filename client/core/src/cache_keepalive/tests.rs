use super::*;
use serde_json::json;

const HEX_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEX_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HEX_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

/// An idle Claude session whose last reply came at 1000.
fn listed(extra: Value) -> ViewRow {
    let mut value = json!({
        "name": "claude/sample/review", "cmd": "claude", "created": 1700000000,
        "run_id": "run-1", "conversation_id": "conversation-1", "state": "running",
        "process_state": "running", "activity": "idle", "phase": "idle",
        "process_work": {"count": 0}, "reply_id": "reply-1", "reply_at": 1000.0,
    });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    crate::view::row(&value)
}

fn idle_row() -> ViewRow {
    listed(json!({}))
}

fn on(row: &ViewRow, hours: i64) -> Map<String, Value> {
    let mut saved = Map::new();
    set_hours(&mut saved, &id("", row).unwrap(), hours);
    saved
}

fn sends(effects: &[Effect]) -> Vec<u64> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Send { request, .. } => Some(*request),
            _ => None,
        })
        .collect()
}

fn state<'a>(keepalive: &'a Keepalive, row: &ViewRow) -> &'a State {
    &keepalive.states[&id("", row).unwrap()]
}

#[test]
fn the_identity_is_qts_session_row_key() {
    // sha256 of the compact JSON Qt hashes (SessionRowPreferences::key).
    let row = listed(json!({"reply_id": ""}));
    assert_eq!(
        id("", &row).unwrap(),
        "bab965b2ba9b3b64042e32e927c3c5f1207e38e81f17d3337738139ea0e4d37d"
    );
    assert_eq!(
        id("", &row).unwrap(),
        zerus_desktop_migration::sha256(br#"["","claude","","conversation-1"]"#)
    );
    assert_eq!(
        id("peer", &row).unwrap(),
        "f701547aec238fe9fa6a2c3a3f1555065cc423e6b26809810c56440d64eb6718"
    );
    // Without a conversation: host, cmd, archive, name and creation time.
    let fresh = listed(json!({"conversation_id": ""}));
    assert_eq!(
        id("", &fresh).unwrap(),
        "25a81d2dc6de84b490d7a02924b146a6d5b0303aa9673c246e12f4383a26c16a"
    );
    assert!(id("", &ViewRow::default()).is_none());
}

#[test]
fn hours_read_as_qt_reads_them_and_are_clamped() {
    let saved = parse(&json!({HEX_A: 30, HEX_B: -2, HEX_C: 2.5, "d": "3", "e": 3.0}).to_string());
    assert_eq!(hours(&saved, HEX_A), MAX_HOURS);
    assert_eq!(hours(&saved, HEX_B), 0);
    assert_eq!(hours(&saved, HEX_C), 0, "toInt of a fraction is 0");
    assert_eq!(hours(&saved, "d"), 0, "a string is not a number");
    assert_eq!(hours(&saved, "e"), 3);
    assert_eq!(hours(&saved, "missing"), 0);
    for broken in ["", "[]", "not json", "1"] {
        assert!(parse(broken).is_empty(), "{broken}");
    }
}

#[test]
fn setting_hours_removes_off_clamps_and_keeps_its_bound() {
    let mut saved = Map::new();
    assert!(set_hours(&mut saved, HEX_A, 30));
    assert_eq!(saved[HEX_A], json!(MAX_HOURS));
    assert!(
        !set_hours(&mut saved, HEX_A, 24),
        "the same value is no change"
    );
    assert!(set_hours(&mut saved, HEX_A, 0));
    assert!(saved.is_empty(), "off removes the key");
    assert!(!set_hours(&mut saved, HEX_A, -1));
    for n in 0..MAX_SESSIONS {
        saved.insert(format!("{n:064x}"), json!(1));
    }
    assert!(!set_hours(&mut saved, HEX_A, 2), "a new key past the bound");
    assert_eq!(saved.len(), MAX_SESSIONS);
    let first = format!("{:064x}", 0);
    assert!(
        set_hours(&mut saved, &first, 5),
        "an existing key still changes"
    );
    assert_eq!(hours(&saved, &first), 5);
    let mut small = Map::new();
    set_hours(&mut small, HEX_B, 2);
    assert_eq!(stored(&small), format!(r#"{{"{HEX_B}":2}}"#));
    assert_eq!(parse(&stored(&small)), small);
    assert_eq!(toggled(0), DEFAULT_HOURS);
    assert_eq!(toggled(6), 0);
}

#[test]
fn the_ttl_comes_from_claudes_native_usage() {
    assert_eq!(ttl(&json!({})), DEFAULT_TTL);
    assert_eq!(
        ttl(&json!({"session_usage": {"prompt_cache": {"ttl_seconds": 0}}})),
        DEFAULT_TTL
    );
    assert_eq!(
        ttl(&json!({"session_usage": {"prompt_cache": {"ttl_seconds": 3600}}})),
        3600.0
    );
}

#[test]
fn nothing_is_sent_for_a_session_that_was_not_turned_on() {
    let mut keepalive = Keepalive::new();
    let row = idle_row();
    let building = listed(json!({"session_state": "build", "build": {"since": 1.0}}));
    for now in [1000.0, 1250.0, 1300.0, 5000.0] {
        assert!(keepalive
            .observe("", &[row.clone()], &Map::new(), now)
            .is_empty());
        let other = on(&listed(json!({"conversation_id": "another"})), 1);
        assert!(keepalive
            .observe("", &[row.clone(), building.clone()], &other, now)
            .is_empty());
    }
    assert!(keepalive.is_empty());
    // Not running, or without a conversation: never warmed.
    let paused = listed(json!({"state": "paused"}));
    assert!(keepalive
        .observe("", &[paused.clone()], &on(&paused, 1), 1250.0)
        .is_empty());
    let unbound = listed(json!({"conversation_id": ""}));
    assert!(keepalive
        .observe("", &[unbound.clone()], &on(&unbound, 1), 1250.0)
        .is_empty());
    assert!(keepalive.is_empty());
}

#[test]
fn a_keepalive_goes_shortly_before_the_cache_expires() {
    let mut keepalive = Keepalive::new();
    let row = listed(json!({"cmd": "codex"}));
    let saved = on(&row, 1);
    // TTL 300, margin min(90, 100): the window opens at 1000 + 300 - 90.
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 1100.0)
        .is_empty());
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 1209.0)
        .is_empty());
    let effects = keepalive.observe("", &[row.clone()], &saved, 1210.0);
    assert_eq!(
        effects,
        vec![Effect::Send {
            request: 1,
            target: Target {
                computer: String::new(),
                name: "claude/sample/review".into(),
                run_id: "run-1".into(),
                conversation_id: "conversation-1".into(),
            },
        }]
    );
    assert!(keepalive.pending(&id("", &row).unwrap()));
    assert_eq!(keepalive.in_flight(), 1);
    // In flight: nothing more is sent.
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 1215.0)
        .is_empty());
    keepalive.sent(1);
    assert_eq!(keepalive.in_flight(), 0);
    // A short TTL takes a third of it as its margin.
    let mut short = Keepalive::new();
    let quick = listed(json!({"cmd": "codex"}));
    short.observe("", &[quick.clone()], &saved, 1000.0);
    short.states.get_mut(&id("", &quick).unwrap()).unwrap().ttl = 60.0;
    assert!(short
        .observe("", &[quick.clone()], &saved, 1039.0)
        .is_empty());
    assert_eq!(sends(&short.observe("", &[quick], &saved, 1040.0)), vec![1]);
}

#[test]
fn an_unanswered_keepalive_counts_as_done_after_twenty_idle_seconds() {
    let mut keepalive = Keepalive::new();
    let row = listed(json!({"cmd": "codex"}));
    let saved = on(&row, 1);
    assert_eq!(
        sends(&keepalive.observe("", &[row.clone()], &saved, 1210.0)),
        vec![1]
    );
    keepalive.sent(1);
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 1229.0)
        .is_empty());
    assert!(keepalive.pending(&id("", &row).unwrap()));
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 1230.0)
        .is_empty());
    assert!(!keepalive.pending(&id("", &row).unwrap()));
    assert_eq!(state(&keepalive, &row).warmed, 1210.0);
    assert!(!state(&keepalive, &row).cold);
    // The next one follows the new warming.
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 1419.0)
        .is_empty());
    assert_eq!(
        sends(&keepalive.observe("", &[row], &saved, 1420.0)),
        vec![2]
    );
}

#[test]
fn an_unanswered_keepalive_marks_the_cache_cold_after_ten_minutes() {
    let mut keepalive = Keepalive::new();
    let row = listed(json!({"cmd": "codex"}));
    let saved = on(&row, 8);
    assert_eq!(
        sends(&keepalive.observe("", &[row.clone()], &saved, 1210.0)),
        vec![1]
    );
    // Never idle again: a tool keeps running.
    let working = listed(json!({"cmd": "codex", "phase": "tool"}));
    assert!(keepalive
        .observe("", &[working.clone()], &saved, 1500.0)
        .is_empty());
    assert!(keepalive.pending(&id("", &row).unwrap()));
    assert!(keepalive
        .observe("", &[working.clone()], &saved, 1810.0)
        .is_empty());
    assert!(keepalive.pending(&id("", &row).unwrap()));
    assert!(keepalive.observe("", &[working], &saved, 1811.0).is_empty());
    assert!(!keepalive.pending(&id("", &row).unwrap()));
    assert!(state(&keepalive, &row).cold);
    // A cold cache is never revived without a real turn.
    for now in [1812.0, 2000.0, 3000.0] {
        assert!(keepalive
            .observe("", &[row.clone()], &saved, now)
            .is_empty());
    }
}

#[test]
fn a_failed_send_is_never_resent() {
    let mut keepalive = Keepalive::new();
    let row = listed(json!({"cmd": "codex"}));
    let saved = on(&row, 1);
    assert_eq!(
        sends(&keepalive.observe("", &[row.clone()], &saved, 1210.0)),
        vec![1]
    );
    keepalive.failed(1);
    assert_eq!(keepalive.in_flight(), 0);
    assert!(state(&keepalive, &row).cold);
    for now in [1211.0, 1250.0, 1290.0] {
        assert!(keepalive
            .observe("", &[row.clone()], &saved, now)
            .is_empty());
    }
    // A late or repeated failure of an older request changes nothing.
    keepalive.failed(1);
    keepalive.failed(7);
    // The next real turn starts again.
    let replied = listed(json!({"cmd": "codex", "reply_id": "reply-2", "reply_at": 2000.0}));
    assert!(keepalive
        .observe("", &[replied.clone()], &saved, 2001.0)
        .is_empty());
    assert!(!state(&keepalive, &replied).cold);
    assert_eq!(
        sends(&keepalive.observe("", &[replied], &saved, 2210.0)),
        vec![2]
    );
}

#[test]
fn a_cache_past_its_lifetime_turns_cold_and_is_never_revived() {
    let mut keepalive = Keepalive::new();
    let row = listed(json!({"cmd": "codex"}));
    let saved = on(&row, 1);
    // First seen (turned on) after the cache expired.
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 1300.0)
        .is_empty());
    assert!(state(&keepalive, &row).cold);
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 1301.0)
        .is_empty());
    // A turn in progress uses the cache again.
    let busy = listed(json!({"cmd": "codex", "activity": "busy", "phase": "tool"}));
    assert!(keepalive.observe("", &[busy], &saved, 1400.0).is_empty());
    assert!(!state(&keepalive, &row).cold);
    assert_eq!(state(&keepalive, &row).real, 1400.0);
    assert_eq!(
        sends(&keepalive.observe("", &[row], &saved, 1610.0)),
        vec![1]
    );
}

#[test]
fn warming_stops_after_the_chosen_hours_without_a_turn() {
    for (limit, expected) in [(1, 0), (2, 1)] {
        let mut keepalive = Keepalive::new();
        let row = listed(json!({"cmd": "codex"}));
        let saved = on(&row, limit);
        keepalive.observe("", &[row.clone()], &saved, 1000.0);
        // Warmed again and again, but the last real turn was at 1000.
        keepalive
            .states
            .get_mut(&id("", &row).unwrap())
            .unwrap()
            .warmed = 4400.0;
        assert_eq!(
            sends(&keepalive.observe("", &[row], &saved, 4610.0)).len(),
            expected,
            "{limit} h"
        );
    }
}

#[test]
fn a_reply_hgs_counts_while_a_keepalive_is_pending_is_marked_read() {
    let mut keepalive = Keepalive::new();
    let row = listed(json!({"cmd": "kimi"}));
    let saved = on(&row, 1);
    assert_eq!(
        sends(&keepalive.observe("", &[row.clone()], &saved, 1210.0)),
        vec![1]
    );
    keepalive.sent(1);
    let answer = listed(json!({"cmd": "kimi", "reply_id": "reply-2", "reply_at": 1214.0}));
    let effects = keepalive.observe("", &[answer.clone()], &saved, 1215.0);
    assert_eq!(
        effects,
        vec![Effect::Read(ReadMark {
            computer: String::new(),
            name: "claude/sample/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conversation-1".into(),
            reply_id: Some("reply-2".into()),
            attention_signature: None,
        })]
    );
    assert!(!keepalive.pending(&id("", &answer).unwrap()));
    assert_eq!(state(&keepalive, &answer).warmed, 1214.0);
    assert_eq!(state(&keepalive, &answer).real, 1000.0, "not a real turn");
    // A reply from before the keepalive is a real turn, never consumed.
    let mut other = Keepalive::new();
    other.observe("", &[row.clone()], &saved, 1210.0);
    let earlier = listed(json!({"cmd": "kimi", "reply_id": "reply-2", "reply_at": 1100.0}));
    assert!(other
        .observe("", &[earlier.clone()], &saved, 1211.0)
        .is_empty());
    assert_eq!(state(&other, &earlier).real, 1100.0);
}

#[test]
fn a_build_warms_every_thirty_minutes_whatever_the_hours() {
    let mut keepalive = Keepalive::new();
    let row = listed(
        json!({"cmd": "codex", "session_state": "build", "build": {"since": 5000.0},
        "process_work": {"count": 1}}),
    );
    let saved = on(&row, 1);
    // Far past the hours and the cache lifetime: the build still warms.
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 6799.0)
        .is_empty());
    assert!(!state(&keepalive, &row).cold);
    assert_eq!(
        sends(&keepalive.observe("", &[row.clone()], &saved, 6800.0)),
        vec![1]
    );
    keepalive.sent(1);
    // Answered once idle (the build counts as idle) for 20 seconds.
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 6820.0)
        .is_empty());
    assert_eq!(state(&keepalive, &row).warmed, 6800.0);
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 8599.0)
        .is_empty());
    assert_eq!(
        sends(&keepalive.observe("", &[row], &saved, 8600.0)),
        vec![2]
    );
    // A build without a known start counts from now.
    let mut fresh = Keepalive::new();
    let started = listed(
        json!({"cmd": "codex", "session_state": "build", "reply_at": 0.0,
        "reply_id": ""}),
    );
    assert!(fresh
        .observe("", &[started.clone()], &saved, 9000.0)
        .is_empty());
    assert_eq!(state(&fresh, &started).warmed, 9000.0);
    assert!(fresh
        .observe("", &[started.clone()], &saved, 10799.0)
        .is_empty());
    assert_eq!(
        sends(&fresh.observe("", &[started], &saved, 10800.0)),
        vec![1]
    );
}

#[test]
fn a_session_that_is_not_idle_is_not_warmed() {
    let busy_child = json!({"coordination_wait": {"schema": 1, "status": "ok", "run_id": "run-1",
        "conversation_id": "conversation-1", "count": 1}});
    for extra in [
        json!({"process_work": {"count": 2}}),
        json!({"subagent_active_count": 1}),
        busy_child,
        json!({"phase": "approval"}),
        json!({"phase": "tool"}),
        json!({"process_state": "exited"}),
        json!({"activity": "unknown"}),
    ] {
        let mut keepalive = Keepalive::new();
        let mut fields = extra.as_object().unwrap().clone();
        fields.insert("cmd".into(), json!("codex"));
        let row = listed(Value::Object(fields));
        assert!(!idle(&row), "{extra}");
        let saved = on(&row, 1);
        for now in [1210.0, 1250.0] {
            assert!(
                keepalive
                    .observe("", &[row.clone()], &saved, now)
                    .is_empty(),
                "{extra}"
            );
        }
    }
    assert!(
        idle(&listed(json!({"process_work": {}}))),
        "an unknown job count is idle"
    );
}

#[test]
fn claudes_inspection_sets_the_ttl_and_the_last_cache_use() {
    let mut keepalive = Keepalive::new();
    let row = idle_row();
    // Two hours, so the hour-long cache below is still within them.
    let saved = on(&row, 2);
    let effects = keepalive.observe("", &[row.clone()], &saved, 1100.0);
    assert_eq!(
        effects,
        vec![Effect::Inspect {
            target: Target {
                computer: String::new(),
                name: "claude/sample/review".into(),
                run_id: "run-1".into(),
                conversation_id: "conversation-1".into(),
            },
        }]
    );
    // One inspection per real turn.
    assert!(keepalive
        .observe("", &[row.clone()], &saved, 1101.0)
        .is_empty());
    // Another conversation's answer changes nothing.
    keepalive.inspected(
        "",
        "claude/sample/review",
        &json!({"conversation_id": "other",
        "session_usage": {"prompt_cache": {"ttl_seconds": 3600}}}),
    );
    assert_eq!(state(&keepalive, &row).ttl, DEFAULT_TTL);
    keepalive.observe("", &[row.clone()], &saved, 1102.0);
    let mut inspected = Keepalive::new();
    inspected.observe("", &[row.clone()], &saved, 1100.0);
    inspected.inspected(
        "",
        "claude/sample/review",
        &json!({"conversation_id": "conversation-1",
        "session_usage": {"prompt_cache": {"ttl_seconds": 3600, "expires_at": 4700.0}}}),
    );
    assert_eq!(state(&inspected, &row).ttl, 3600.0);
    assert_eq!(
        state(&inspected, &row).warmed,
        1100.0,
        "max(1000, 4700 - 3600)"
    );
    // The hour-long cache is warmed 90 s before it expires.
    assert!(inspected
        .observe("", &[row.clone()], &saved, 4609.0)
        .is_empty());
    assert_eq!(
        sends(&inspected.observe("", &[row.clone()], &saved, 4610.0)),
        vec![1]
    );
    // A failed inspection is not repeated before the next real turn.
    let mut failed = Keepalive::new();
    failed.observe("", &[row.clone()], &saved, 1100.0);
    failed.inspection_failed("", "claude/sample/review");
    assert!(failed
        .observe("", &[row.clone()], &saved, 1101.0)
        .is_empty());
    // Other agents keep the default TTL without an inspection.
    let codex = listed(json!({"cmd": "codex"}));
    assert!(Keepalive::new()
        .observe("", &[codex.clone()], &on(&codex, 1), 1100.0)
        .is_empty());
}

#[test]
fn a_new_run_or_conversation_starts_over_and_unlisted_sessions_are_forgotten() {
    let mut keepalive = Keepalive::new();
    let row = listed(json!({"cmd": "codex"}));
    let saved = on(&row, 1);
    keepalive.observe("", &[row.clone()], &saved, 1300.0);
    assert!(state(&keepalive, &row).cold);
    // Resumed: the cache was last used by the last reply.
    let resumed = listed(json!({"cmd": "codex", "run_id": "run-2", "reply_at": 1250.0}));
    assert!(keepalive
        .observe("", &[resumed.clone()], &saved, 1300.0)
        .is_empty());
    assert!(!state(&keepalive, &resumed).cold);
    assert_eq!(state(&keepalive, &resumed).run, "run-2");
    assert_eq!(
        sends(&keepalive.observe("", &[resumed.clone()], &saved, 1460.0)),
        vec![1]
    );
    // A peer's listing keeps this computer's states; this computer's drops it.
    let peer = listed(json!({"cmd": "codex", "name": "codex/peer/one"}));
    keepalive.observe("peer", &[peer.clone()], &on(&peer, 1), 1500.0);
    assert!(keepalive.states.contains_key(&id("", &resumed).unwrap()));
    keepalive.observe("", &[], &saved, 1500.0);
    assert!(!keepalive.states.contains_key(&id("", &resumed).unwrap()));
    // Turning it off forgets the session as well.
    keepalive.observe("", &[resumed.clone()], &saved, 1600.0);
    keepalive.observe("", &[resumed.clone()], &on(&resumed, 0), 1601.0);
    assert!(!keepalive.states.contains_key(&id("", &resumed).unwrap()));
}
