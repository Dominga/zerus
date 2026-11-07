//! Task 184: the secret scan of docs/lore.md 11.9 in the hgs working tier.
//! The scanner is a copy of the lore crate's, so both run the same vectors from
//! `tests/fixtures/secret-scan-vectors.json`; here the working-tier writes are
//! redacted, never refused.
use super::*;
use crate::state::teams::handoff;
use crate::state::teams::model::Team;
use crate::state::teams::pulses::Pulse;
use crate::state::teams::store::Store;
use crate::state::teams::working::{self, NewMessage, Urgency};
use serde_json::{json, Value};

fn vectors() -> Value {
    serde_json::from_str(include_str!("../../../../tests/fixtures/secret-scan-vectors.json")).expect("the shared secret vectors")
}

#[test]
fn the_shared_vectors_are_found_or_clean() {
    let vectors = vectors();
    for case in vectors["found"].as_array().expect("found vectors") {
        let text = case["text"].as_str().unwrap();
        let pattern = case["pattern"].as_str().unwrap();
        assert_eq!(scan(text, &[]).first().map(|found| found.pattern), Some(pattern), "{text}");
    }
    for case in vectors["clean"].as_array().expect("clean vectors") {
        let text = case["text"].as_str().unwrap();
        assert_eq!(scan(text, &[]), Vec::new(), "{text}");
    }
}

#[test]
fn a_working_text_is_redacted_not_refused() {
    let text = "curl -H 'Authorization: Bearer ghp_0123456789abcdefghijABCDEFGHIJ012345' https://admin:hunter2pass@example.invalid";
    let clean = redact_working(text);
    assert!(clean.contains("[redacted: github-token]"), "{clean}");
    assert!(clean.contains("[redacted: url-credentials]"), "{clean}");
    assert!(!clean.contains("0123456789abcdefghijABCDEFGHIJ") && !clean.contains("hunter2pass"), "{clean}");
    assert_eq!(redact_working("nothing secret here"), "nothing secret here");
    // A redacted text scans clean, so the store can redact on every write.
    assert_eq!(scan(&clean, &[]), Vec::new(), "{clean}");
}

fn stored_team(path: &std::path::Path) -> (Store, Team) {
    let mut store = Store::open_at(path, true).unwrap();
    let team = Team { id: "t".into(), name: "T".into(), ..Default::default() };
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    (store, team)
}

#[test]
fn a_pulse_is_stored_redacted() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let (store, team) = stored_team(&dir.path().join("teams/teams.sqlite3"));
        let pulse = Pulse {
            team: team.id.clone(),
            member: "dev".into(),
            task: 1,
            done: "curl -H 'Authorization: Bearer ghp_0123456789abcdefghijABCDEFGHIJ012345'".into(),
            next: "last: Bash: DB_PASSWORD=Tr0ub4dor&3x".into(),
            blocked: "https://admin:hunter2pass@example.invalid failed".into(),
            at: 10.0,
            source: "auto".into(),
            ..Default::default()
        };
        let stored = store.put_pulse(&pulse).unwrap();
        assert!(stored.done.contains("[redacted: github-token]") && !stored.done.contains("ghp_0123"), "{}", stored.done);
        assert!(stored.next.contains("[redacted: secret-assignment]") && !stored.next.contains("Tr0ub4dor"), "{}", stored.next);
        assert!(stored.blocked.contains("[redacted: url-credentials]") && !stored.blocked.contains("hunter2pass"), "{}", stored.blocked);
        // The same redaction is what the lead and the digest read.
        let latest = store.latest_pulses(&team.id, 20.0);
        assert_eq!(latest.len(), 1);
        assert!(latest[0].next.contains("[redacted: secret-assignment]"), "{}", latest[0].next);
    });
}

#[test]
fn a_team_message_is_stored_redacted() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let (mut store, team) = stored_team(&dir.path().join("teams/teams.sqlite3"));
        let body = "rotate AKIAABCDEFGHIJKLWXYZ in the vault";
        let stored = store
            .mutate_working(&team.id, |tx, _team, _events| {
                let new = NewMessage { request_id: "22222222-2222-4222-8222-222222222222", by: "lead", to: "dev", kind: "message", urgency: Urgency::Normal, text: body };
                let (message, _) = working::post(tx, "t", &new, 100.0)?;
                Ok(message)
            })
            .unwrap();
        assert!(stored.text.contains("[redacted: aws-access-key-id]") && !stored.text.contains("AKIAABCDEFGHIJKLWXYZ"), "{}", stored.text);
        // The digest is of the redacted text, so the same request stays idempotent.
        let again = store
            .mutate_working(&team.id, |tx, _team, _events| {
                let new = NewMessage { request_id: "22222222-2222-4222-8222-222222222222", by: "lead", to: "dev", kind: "message", urgency: Urgency::Normal, text: body };
                working::post(tx, "t", &new, 100.0)
            })
            .unwrap();
        assert!(!again.1, "the retry is the stored message, not another one");
    });
}

#[test]
fn a_handoff_brief_kept_in_the_team_is_redacted() {
    let mut team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        goal: "ship it with ghp_0123456789abcdefghijABCDEFGHIJ012345".into(),
        ..Default::default()
    };
    team.members.push(crate::state::teams::model::Member { id: "dev".into(), role: "Backend".into(), ..Default::default() });
    let built = handoff::build(&team, "dev", handoff::Why::Compaction, &["ran DB_PASSWORD=Tr0ub4dor&3x".into()], 4_000);
    assert!(built.text.contains("[redacted: github-token]"), "{}", built.text);
    assert!(built.text.contains("[redacted: secret-assignment]"), "{}", built.text);
    assert!(!built.text.contains("ghp_0123456789") && !built.text.contains("Tr0ub4dor"), "{}", built.text);
}
