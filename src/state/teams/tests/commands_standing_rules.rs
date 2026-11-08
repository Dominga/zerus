//! Task #279 (owner, 2026-11-08): the owner's standing rules are kept in
//! CodeLore with their words and source. An owner message of any age is cited
//! by its id in the lead session's journal and a quote, Zerus verifies the
//! quote against that message, and the rule is recorded in the team's decisions
//! (and so its lore) and recalled in the lead brief and in every hand-out it
//! binds.
use super::*;
use super::support::*;

/// The team id every request here names (a canonical UUID, as a team needs).
const TEAM: &str = "33333333-3333-4333-8333-333333333333";

fn owner_message(text: &str, at: f64) -> Value {
    json!({"type":"UserPromptSubmit","at":at,"agent_id":"","detail":text})
}

/// A running team whose lead session is `claude/x/lead` (conversation `c1`),
/// with one task that a rule may name.
fn citing_team(now: f64) -> Team {
    let mut team = Team {
        id: TEAM.into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        created: now - 4000.0,
        ..Default::default()
    };
    team.members.push(Member {
        id: "lead".into(),
        role: "Lead".into(),
        agent: "claude".into(),
        session: Some(SessionRef { name: "claude/x/lead".into(), conversation_id: "lead-conversation".into(), ..Default::default() }),
        ..Default::default()
    });
    team.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() });
    team.tasks.push(Task { id: 1, title: "Tauri teams".into(), status: TaskStatus::Pending, assignee: "dev".into(), ..Default::default() });
    team
}

fn events_db(dir: &std::path::Path, rows: &[(i64, f64, &str)]) {
    use rusqlite::Connection;
    let db = Connection::open(dir.join("events.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY, name TEXT, conversation TEXT, payload TEXT);").unwrap();
    for (seq, at, text) in rows {
        db.execute(
            "INSERT INTO events(seq, name, conversation, payload) VALUES (?1, 'claude/x/lead', 'lead-conversation', ?2)",
            rusqlite::params![seq, owner_message(text, *at).to_string()],
        )
        .unwrap();
    }
}

fn stored_team() -> Team {
    Store::open(false).unwrap().load(TEAM).unwrap().unwrap()
}

#[test]
fn an_old_owner_message_is_cited_and_verified_at_any_age() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let now = crate::state::now();
        // Ten days old: far beyond the 30-minute window of a typed word.
        let old = now - 10.0 * 86_400.0;
        events_db(dir.path(), &[(1, old, "1) No git push, tag, release or PR until I allow it; it waits for full Tauri parity.")]);
        Store::open(true).unwrap().create(TEAM, &json!({}), citing_team(now), vec![]).unwrap();
        // A canonical request id, as every team request needs.
        let request = "11111111-1111-4111-8111-111111111111";
        let recorded = as_lead(|| super::decide_command(&json!({"request_id":request,"team":TEAM,
            "question":"May anyone push before Tauri parity?","choice":"No push, tag, release or PR until the owner allows it (full Tauri parity)",
            "why":"Confirm all","kind":"standing rule","tasks":[1],
            "owner_message":{"id":1,"quote":"No git push, tag, release or PR until I allow it"}})))
        .unwrap();
        assert_eq!(recorded["status"], "recorded", "{recorded}");
        let decision = stored_team().decisions.into_iter().find(|found| found.id == recorded["decision"]["id"].as_str().unwrap_or_default()).unwrap();
        assert_eq!(decision.kind, "rule");
        assert!(decision.by.contains("your message 1"), "{}", decision.by);
        assert_eq!(decision.owner_message.as_ref().map(|message| message["at"].as_f64()), Some(Some(old)));
        // The rules block of the brief and the hand-out name it with its source.
        let team = stored_team();
        let brief = super::super::dispatcher::brief_text(&team);
        assert!(brief.contains("standing rule d1") && brief.contains("From your message 1") && brief.contains("No git push"), "{brief}");
        let block = super::super::decisions::block(&team, team.task(1).unwrap());
        assert!(block.contains("standing rule d1"), "{block}");
        // The lead can find the id to cite.
        let listed = super::decisions_command(&json!({"team":TEAM,"action":"owner-messages"})).unwrap();
        assert_eq!(listed["messages"][0]["id"], 1, "{listed}");
        assert!(listed["messages"][0]["text"].as_str().unwrap_or_default().contains("No git push"), "{listed}");
    });
}

#[test]
fn a_wrong_quote_or_another_message_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let now = crate::state::now();
        let old = now - 10.0 * 86_400.0;
        events_db(
            dir.path(),
            &[(1, old, "Never touch the working GUI while I test it."), (2, old + 60.0, "Phase 9 gets every missing Qt feature.")],
        );
        Store::open(true).unwrap().create(TEAM, &json!({}), citing_team(now), vec![]).unwrap();
        let ask = |message: Value| {
            super::decide_command(&json!({"request_id":"22222222-2222-4222-8222-222222222222","team":TEAM,
                "question":"A rule?","choice":"Yes","kind":"rule","owner_message":message}))
        };
        // A quote that the message does not have.
        let error = as_lead(|| ask(json!({"id":1,"quote":"touch the working GUI freely"}))).unwrap_err().to_string();
        assert!(error.contains("does not contain that quote"), "{error}");
        // A quote from another message, cited by this one's id.
        let error = as_lead(|| ask(json!({"id":1,"quote":"Phase 9 gets every missing Qt feature"}))).unwrap_err().to_string();
        assert!(error.contains("does not contain that quote"), "{error}");
        // An id that is not an owner message.
        let error = as_lead(|| ask(json!({"id":999,"quote":"Never touch the working GUI"}))).unwrap_err().to_string();
        assert!(error.contains("is not in the lead session's journal"), "{error}");
        // The same quote in the right message, with other case and spacing.
        let recorded = as_lead(|| ask(json!({"id":1,"quote":"never   touch the working   gui"}))).unwrap();
        assert_eq!(recorded["status"], "recorded", "{recorded}");
    });
}



