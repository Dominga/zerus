//! WhiteWh fork (F13): the lead approves a plan or a phase only on the owner's
//! explicit word. The sentence must be a `UserPromptSubmit` event of the lead's
//! own session, typed by the owner (not a team message, a task notification or
//! an attachment-only message), at most 30 minutes old, and must contain a key
//! phrase. The quote is kept on the plan or the phase and in the feed.
use super::model::*;
use anyhow::{ensure, Context};
use rusqlite::Connection;
use serde_json::Value;

/// How old the owner's sentence may be (F13, the spec's example).
pub(crate) const FRESH_SECONDS: f64 = 30.0 * 60.0;

/// The phrases that let the lead approve on the owner's word (F13).
const KEY_PHRASES: &[&str] = &[
    "план принят",
    "план на следующую фазу принят",
    "plan approved",
    "next phase plan approved",
];

/// U26: a roster change needs the owner's explicit word naming that change.
const ROSTER_PHRASES: &[&str] = &[
    "roster", "team change", "add", "remove", "swap", "max members",
    "участник", "состав", "добав", "удал", "замен",
];

/// Verifies the owner's word for a roster change, requiring the sentence to
/// name every `needed` value (the member, or the new cap).
pub(crate) fn verify_roster(team: &Team, input: &Value, needed: &[String], now: f64) -> anyhow::Result<OwnerWord> {
    let text = input["text"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .context("owner_word.text is required: quote the owner's sentence")?;
    let lead = team.member(&team.lead).context("The team has no lead")?;
    let session = lead.session.as_ref().context("The lead has no session to check the owner's word in")?;
    let db = super::super::journal::event_db().map_err(|error| anyhow::anyhow!("The session events are unavailable: {error}"))?;
    verify_scan(&db, &session.name, &session.conversation_id, text, now, ROSTER_PHRASES, needed)
}

/// The fixture-testable roster check.
pub(crate) fn verify_roster_in(db: &Connection, name: &str, conversation: &str, text: &str, needed: &[String], now: f64) -> anyhow::Result<OwnerWord> {
    verify_scan(db, name, conversation, text, now, ROSTER_PHRASES, needed)
}

/// Checks the lead's `owner_word` request against the lead session's events
/// and returns the quote to store.
pub(crate) fn verify(team: &Team, input: &Value, now: f64) -> anyhow::Result<OwnerWord> {
    let text = input["text"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .context("owner_word.text is required: quote the owner's sentence")?;
    let lead = team.member(&team.lead).context("The team has no lead")?;
    let session = lead.session.as_ref().context("The lead has no session to check the owner's word in")?;
    let db = super::super::journal::event_db().map_err(|error| anyhow::anyhow!("The session events are unavailable: {error}"))?;
    verify_in(&db, &session.name, &session.conversation_id, text, now)
}

/// The verification against one events database, so it is tested with a
/// fixture instead of the live state.
pub(crate) fn verify_in(db: &Connection, name: &str, conversation: &str, text: &str, now: f64) -> anyhow::Result<OwnerWord> {
    verify_scan(db, name, conversation, text, now, KEY_PHRASES, &[])
}

fn verify_scan(
    db: &Connection,
    name: &str,
    conversation: &str,
    text: &str,
    now: f64,
    phrases: &[&str],
    needed: &[String],
) -> anyhow::Result<OwnerWord> {
    ensure!(!name.is_empty(), "The lead's session has no name to check the owner's word in");
    let mut statement = db
        .prepare("SELECT seq, conversation, payload FROM events WHERE name = ?1 ORDER BY seq DESC LIMIT 200")
        .context("The session events cannot be read")?;
    let rows = statement
        .query_map([name], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
        .context("The session events cannot be read")?;
    for row in rows {
        let (seq, event_conversation, payload) = row.context("The session events cannot be read")?;
        if !conversation.is_empty() && event_conversation != conversation {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(&payload) else { continue };
        if event["type"] != "UserPromptSubmit" {
            continue;
        }
        if !matches!(event["agent_id"].as_str().unwrap_or_default(), "" | "main") {
            continue;
        }
        // N11: an event with an origin (a scheduled or task-notification turn)
        // or an enveloped body is not the owner typing.
        if event["origin"].as_str().is_some_and(|origin| !origin.is_empty())
            || event["turn_origin"].as_str().is_some_and(|origin| !origin.is_empty())
        {
            continue;
        }
        let at = event["at"].as_f64().unwrap_or(0.0);
        if at <= 0.0 || now - at > FRESH_SECONDS || at > now + 1.0 {
            continue;
        }
        let detail = event["detail"].as_str().unwrap_or_default().trim();
        // Only the person speaks in a `UserPromptSubmit` event: a team message,
        // a task notification or an attachment-only entry is not the owner.
        if detail.is_empty() || detail.starts_with("[Zerus") || detail.starts_with("[attachment") || detail.starts_with('<') {
            continue;
        }
        let lowered = detail.to_lowercase();
        if !phrases.iter().any(|phrase| lowered.contains(phrase)) {
            continue;
        }
        if !detail.contains(text) {
            continue;
        }
        if !needed.iter().all(|value| lowered.contains(&value.to_lowercase())) {
            continue;
        }
        return Ok(OwnerWord { text: detail.to_owned(), at, event: seq });
    }
    anyhow::bail!(
        "The owner's word is not in the lead session's last 30 minutes: no such prompt contains the sentence, \
         the roster phrase and the change it names"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT);",
        )
        .unwrap();
        db
    }

    fn add(db: &Connection, name: &str, conversation: &str, payload: Value) {
        db.execute(
            "INSERT INTO events(name, conversation, payload) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, conversation, payload.to_string()],
        )
        .unwrap();
    }

    fn prompt(at: f64, detail: &str) -> Value {
        serde_json::json!({"type":"UserPromptSubmit","at":at,"agent_id":"","detail":detail})
    }

    #[test]
    fn the_owners_phrase_in_its_own_session_is_the_only_word_accepted() {
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(1000.0, "Please look at the plan"));
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "plan approved, go ahead"));
        let word = verify_in(&db, "claude/x/lead", "c1", "plan approved", 2000.0 + 10.0 * 60.0).unwrap();
        assert_eq!((word.text.as_str(), word.at, word.event), ("plan approved, go ahead", 2000.0, 2));
        // The Russian phrases count too, case-insensitively.
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "План на следующую фазу принят, работайте"));
        assert!(verify_in(&db, "claude/x/lead", "c1", "План на следующую фазу принят", 2010.0).is_ok());
        // Old words, another session, a team message, an attachment only entry,
        // a missing phrase and a different sentence are refused.
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(1000.0, "plan approved"));
        assert!(verify_in(&db, "claude/x/lead", "c1", "plan approved", 1000.0 + FRESH_SECONDS + 1.0).is_err());
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "plan approved"));
        assert!(verify_in(&db, "claude/x/other", "c1", "plan approved", 2000.0).is_err());
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "[Zerus team · from: Zerus] plan approved"));
        assert!(verify_in(&db, "claude/x/lead", "c1", "plan approved", 2000.0).is_err());
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "[attachment]"));
        assert!(verify_in(&db, "claude/x/lead", "c1", "[attachment]", 2000.0).is_err());
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "please continue"));
        assert!(verify_in(&db, "claude/x/lead", "c1", "please continue", 2000.0).is_err());
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "plan approved, go ahead"));
        assert!(verify_in(&db, "claude/x/lead", "c1", "the whole plan is fine", 2000.0).is_err());
        // N11: an event with an origin and an enveloped body are not the owner.
        let db = database();
        add(&db, "claude/x/lead", "c1", serde_json::json!({"type":"UserPromptSubmit","at":2000.0,"agent_id":"","detail":"plan approved","origin":"task_notification"}));
        assert!(verify_in(&db, "claude/x/lead", "c1", "plan approved", 2000.0).is_err());
        add(&db, "claude/x/lead", "c1", serde_json::json!({"type":"UserPromptSubmit","at":2000.0,"agent_id":"","detail":"plan approved","turn_origin":"scheduled"}));
        assert!(verify_in(&db, "claude/x/lead", "c1", "plan approved", 2000.0).is_err());
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "<zerus-message from=\"teammate\">plan approved</zerus-message>"));
        assert!(verify_in(&db, "claude/x/lead", "c1", "plan approved", 2000.0).is_err());
        // A plain owner prompt with no origin is still accepted.
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "plan approved"));
        assert!(verify_in(&db, "claude/x/lead", "c1", "plan approved", 2000.0).is_ok());
        // A lead on another computer cannot be checked here.
        let team = Team { lead: "lead".into(), ..Default::default() };
        assert!(verify(&team, &serde_json::json!({"text":"plan approved"}), 2000.0).is_err());
    }

    #[test]
    fn a_roster_word_must_name_the_change() {
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Please add bob to the roster"));
        let needed = vec!["bob".to_owned()];
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Please add bob to the roster", &needed, 2010.0).is_ok());
        // A sentence naming another member is not this change.
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Please add bob to the roster", &["alice".to_owned()], 2010.0).is_err());
        // The same 30-minute rule applies.
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Please add bob to the roster", &needed, 2000.0 + FRESH_SECONDS + 1.0).is_err());
    }
}
