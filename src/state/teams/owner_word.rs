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
pub(crate) const ROSTER_PHRASES: &[&str] = &[
    "roster", "team change", "member", "members", "участник", "состав",
];

/// U49: the words that ask a member to drain, English and Russian.
pub(crate) const DRAIN_PHRASES: &[&str] = &["drain", "remove", "вывед", "убери", "убрать"];

/// U32: what the owner's sentence must contain for one roster change: one of
/// `phrases` (for an add, the verb "add"/"добав" instead), and every token in
/// `all` (the operation and the member or cap it names).
#[derive(Clone, Debug, Default)]
pub(crate) struct WordRule {
    pub phrases: &'static [&'static str],
    pub all: Vec<String>,
    pub add: bool,
}

impl WordRule {
    /// Whether this change needs the owner's word at all.
    pub(crate) fn needed(&self) -> bool {
        self.add || !self.all.is_empty()
    }
}

/// U32: the words a count may be written with, English and Russian.
const NUMBER_WORDS: &[(&str, usize)] = &[
    ("one", 1), ("two", 2), ("three", 3), ("four", 4), ("five", 5), ("six", 6), ("seven", 7),
    ("eight", 8), ("nine", 9), ("ten", 10), ("один", 1), ("одного", 1), ("одну", 1), ("два", 2),
    ("две", 2), ("двух", 2), ("три", 3), ("трех", 3), ("трёх", 3), ("четыре", 4), ("четырех", 4),
    ("пять", 5), ("шесть", 6), ("семь", 7), ("восемь", 8), ("девять", 9), ("десять", 10),
];

/// U32: the role aliases the owner may use for a member, English and Russian.
const ROLE_ALIASES: &[(&[&str], &[&str])] = &[
    (&["backend", "бэкенд"], &["backend", "бэкенд", "бэкендер"]),
    (&["frontend", "фронт"], &["frontend", "фронт", "фронтенд"]),
    (&["qa", "tester", "test", "тестер", "тестировщик"], &["qa", "test", "тест", "тестер", "tester"]),
    (&["reviewer", "review", "ревью"], &["reviewer", "review", "ревью", "ревьюер"]),
    (&["expert", "эксперт"], &["expert", "эксперт"]),
];

/// U32: the agents the owner may name for a member.
const AGENT_ALIASES: &[(&[&str], &[&str])] = &[
    (&["dsh", "deepseek"], &["deepseek", "дипсик", "dsh"]),
    (&["claude"], &["claude", "клод"]),
    (&["codex"], &["codex", "кодекс"]),
    (&["kimi"], &["kimi", "кими"]),
];

/// U32: how many members the owner's sentence asks to add; a number it does not
/// say is one. Bounded to the roster's own limit.
pub(crate) fn add_count(text: &str) -> usize {
    let lowered = text.to_lowercase();
    let mut found = None;
    for token in lowered.split(|ch: char| !ch.is_alphanumeric() && ch != '_') {
        let number = if token.chars().all(|ch| ch.is_ascii_digit()) {
            token.parse::<usize>().ok()
        } else {
            NUMBER_WORDS.iter().find(|(word, _)| *word == token).map(|(_, number)| *number)
        };
        if number.is_some() {
            found = number;
            break;
        }
    }
    found.unwrap_or(1).clamp(1, 32)
}

/// U32: whether the owner's sentence names the member being added by its role
/// or its agent/model, as the owner says them (never by the generated id).
pub(crate) fn names_member(text: &str, role: &str, agent: &str, model: &str) -> bool {
    let lowered = text.to_lowercase();
    let mut aliases: Vec<&'static str> = Vec::new();
    aliases.extend(role_aliases(role));
    aliases.extend(agent_aliases(agent));
    aliases.extend(model_aliases(model));
    aliases.into_iter().any(|alias| alias_in(&lowered, alias))
}

fn role_aliases(role: &str) -> &'static [&'static str] {
    let role = role.to_lowercase();
    ROLE_ALIASES
        .iter()
        .find(|(keys, _)| keys.iter().any(|key| role.contains(key)))
        .map(|(_, aliases)| *aliases)
        .unwrap_or(&[])
}

fn agent_aliases(agent: &str) -> &'static [&'static str] {
    let agent = agent.to_lowercase();
    AGENT_ALIASES
        .iter()
        .find(|(keys, _)| keys.iter().any(|key| agent == *key || agent.contains(key)))
        .map(|(_, aliases)| *aliases)
        .unwrap_or(&[])
}

fn model_aliases(model: &str) -> &'static [&'static str] {
    let model = model.to_lowercase();
    if model.contains("opus") || model.contains("опус") {
        &["opus", "опус"]
    } else if model.contains("sonnet") || model.contains("haiku") || model.contains("claude") {
        &["claude", "клод"]
    } else if model.contains("gpt") || model.contains("codex") {
        &["codex", "кодекс"]
    } else if model.contains("kimi") {
        &["kimi", "кими"]
    } else if model.contains("deepseek") {
        &["deepseek", "дипсик", "dsh"]
    } else {
        &[]
    }
}

/// U32: an English alias is a whole token ("qa" is not in "squad"); a Russian
/// one is matched inside its inflections ("бэкенд" in "бэкендера").
fn alias_in(lowered: &str, alias: &str) -> bool {
    if alias.is_ascii() {
        // U49: a scale names the role in the plural too ("add two backends").
        has_token(lowered, alias) || has_token(lowered, &format!("{alias}s"))
    } else {
        lowered.contains(alias)
    }
}

/// U49: a value the sentence must name; a count may be said as a number word
/// ("add two backends" names 2, as "add 2 backends" does).
fn needed_in(lowered: &str, value: &str) -> bool {
    let value = value.to_lowercase();
    if has_token(lowered, &value) {
        return true;
    }
    let Ok(number) = value.parse::<usize>() else { return false };
    NUMBER_WORDS.iter().any(|(word, n)| *n == number && has_token(lowered, word))
}

/// U26/N10: a whole token, not a substring ("add" does not match "address").
fn has_token(haystack: &str, token: &str) -> bool {
    if token.is_empty() {
        return true;
    }
    let mut from = 0;
    while let Some(at) = haystack[from..].find(token) {
        let start = from + at;
        let end = start + token.len();
        let before = haystack[..start].chars().next_back();
        let after = haystack[end..].chars().next();
        let boundary = |ch: Option<char>| ch.is_none_or(|ch| !ch.is_alphanumeric() && ch != '-' && ch != '_');
        if boundary(before) && boundary(after) {
            return true;
        }
        from = end;
    }
    false
}

/// Verifies the owner's word for a roster change, requiring the sentence to
/// meet `rule`: the roster phrase (or, for an add, the add verb) and every
/// value it names.
pub(crate) fn verify_roster(team: &Team, input: &Value, rule: &WordRule, now: f64) -> anyhow::Result<OwnerWord> {
    let text = input["text"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .context("owner_word.text is required: quote the owner's sentence")?;
    let lead = team.member(&team.lead).context("The team has no lead")?;
    let session = lead.session.as_ref().context("The lead has no session to check the owner's word in")?;
    let db = super::super::journal::event_db().map_err(|error| anyhow::anyhow!("The session events are unavailable: {error}"))?;
    verify_scan(&db, &session.name, &session.conversation_id, text, now, rule_scan(rule, true))
}

/// The fixture-testable roster check.
#[cfg(test)]
pub(crate) fn verify_roster_in(db: &Connection, name: &str, conversation: &str, text: &str, rule: &WordRule, now: f64) -> anyhow::Result<OwnerWord> {
    verify_scan(db, name, conversation, text, now, rule_scan(rule, true))
}

fn rule_scan(rule: &WordRule, exact: bool) -> ScanOptions<'_> {
    ScanOptions { phrases: rule.phrases, needed: &rule.all, exact, add: rule.add, accept: None, after: 0.0 }
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
    verify_scan(db, name, conversation, text, now, ScanOptions { phrases: KEY_PHRASES, needed: &[], exact: false, add: false, accept: None, after: 0.0 })
}

/// U29: the owner asks to add a task to a running phase, naming it by number
/// or as the current one; then confirms the warning in a newer sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhaseWord {
    Request,
    Confirm,
}

/// The phase numbers a sentence names ("phase 3", "фаза 3", "фазу 3").
fn named_phases(lowered: &str) -> Vec<u32> {
    let words: Vec<&str> = lowered.split(|ch: char| !ch.is_alphanumeric()).filter(|word| !word.is_empty()).collect();
    words
        .windows(2)
        .filter(|pair| pair[0] == "phase" || pair[0].starts_with("фаз"))
        .filter_map(|pair| pair[1].parse().ok())
        .collect()
}

fn asks_to_add(lowered: &str, phase: u32) -> bool {
    let adds = has_token(lowered, "add") || lowered.contains("добав");
    let task = lowered.contains("task") || lowered.contains("задач");
    let current = ["current phase", "running phase", "this phase", "текущ", "идущ", "эту фаз", "этой фаз"]
        .iter()
        .any(|phrase| lowered.contains(phrase));
    let names = named_phases(lowered);
    adds && task && (names.contains(&phase) || (names.is_empty() && current))
}

fn confirms(lowered: &str, phase: u32) -> bool {
    let yes = ["add", "confirm", "confirmed", "yes", "ok", "okay", "да"].iter().any(|token| has_token(lowered, token))
        || lowered.contains("подтвер")
        || lowered.contains("добав");
    // A confirmation naming another phase is not this one's.
    yes && named_phases(lowered).iter().all(|n| *n == phase)
}

/// U29: verifies the owner's request (or confirmation) for an addition to
/// running phase `phase`; a confirmation must be newer than `after`.
pub(crate) fn verify_phase(team: &Team, input: &Value, phase: u32, kind: PhaseWord, after: f64, now: f64) -> anyhow::Result<OwnerWord> {
    let text = input["text"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .context("owner_word.text is required: quote the owner's sentence")?;
    let lead = team.member(&team.lead).context("The team has no lead")?;
    let session = lead.session.as_ref().context("The lead has no session to check the owner's word in")?;
    let db = super::super::journal::event_db().map_err(|error| anyhow::anyhow!("The session events are unavailable: {error}"))?;
    verify_phase_in(&db, &session.name, &session.conversation_id, text, phase, kind, after, now)
}

/// The fixture-testable U29 check.
#[allow(clippy::too_many_arguments)]
pub(crate) fn verify_phase_in(
    db: &Connection,
    name: &str,
    conversation: &str,
    text: &str,
    phase: u32,
    kind: PhaseWord,
    after: f64,
    now: f64,
) -> anyhow::Result<OwnerWord> {
    let accept = move |lowered: &str| match kind {
        PhaseWord::Request => asks_to_add(lowered, phase),
        PhaseWord::Confirm => confirms(lowered, phase),
    };
    verify_scan(db, name, conversation, text, now, ScanOptions { phrases: &[], needed: &[], exact: false, add: false, accept: Some(&accept), after })
        .with_context(|| match kind {
            PhaseWord::Request => format!("The owner has not asked to add a task to phase {phase} (name the phase or say the current phase)"),
            PhaseWord::Confirm => format!("The owner has not confirmed the addition to phase {phase} after the warning"),
        })
}

struct ScanOptions<'a> {
    phrases: &'a [&'a str],
    needed: &'a [String],
    exact: bool,
    /// U32: an add request, which says "add"/"добав" instead of a roster phrase.
    add: bool,
    /// U29: a check of the lowered sentence instead of the phrases.
    accept: Option<&'a dyn Fn(&str) -> bool>,
    /// U29: the sentence must be newer than this time.
    after: f64,
}

fn verify_scan(
    db: &Connection,
    name: &str,
    conversation: &str,
    text: &str,
    now: f64,
    options: ScanOptions<'_>,
) -> anyhow::Result<OwnerWord> {
    ensure!(!name.is_empty(), "The lead's session has no name to check the owner's word in");
    ensure!(!text.trim().is_empty(), "owner_word.text is required: quote the owner's sentence");
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
        if at <= 0.0 || now - at > FRESH_SECONDS || at > now + 1.0 || at <= options.after {
            continue;
        }
        let detail = event["detail"].as_str().unwrap_or_default().trim();
        // Only the person speaks in a `UserPromptSubmit` event: a team message,
        // a task notification or an attachment-only entry is not the owner.
        if detail.is_empty() || detail.starts_with("[Zerus") || detail.starts_with("[attachment") || detail.starts_with('<') {
            continue;
        }
        let lowered = detail.to_lowercase();
        let said = if let Some(accept) = options.accept {
            accept(&lowered)
        } else if options.add {
            // U32: the owner asks to add someone, in either language; "add" is
            // a whole token, so "address" is not an add.
            has_token(&lowered, "add") || lowered.contains("добав")
        } else {
            options.phrases.iter().any(|phrase| lowered.contains(phrase))
        };
        if !said {
            continue;
        }
        if if options.exact { detail.trim() != text.trim() } else { !detail.contains(text) } {
            continue;
        }
        if !options.needed.iter().all(|value| needed_in(&lowered, value)) {
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

    fn roster_rule(all: &[&str]) -> WordRule {
        WordRule { phrases: ROSTER_PHRASES, all: all.iter().map(|value| (*value).to_owned()).collect(), add: false }
    }

    fn add_rule() -> WordRule {
        WordRule { phrases: &[], all: Vec::new(), add: true }
    }

    #[test]
    fn a_roster_word_matches_whole_tokens_and_the_operation_verb() {
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Add a QA check so dev can retest"));
        // "add" inside "address" and the wrong operation verb are refused.
        let error = verify_roster_in(&db, "claude/x/lead", "c1", "Add a QA check so dev can retest", &roster_rule(&["remove", "dev"]), 2000.0).unwrap_err().to_string();
        assert!(error.contains("owner's word"), "{error}");
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Address the padding"));
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Address the padding", &roster_rule(&["add", "dev"]), 2000.0).is_err());
        // The add verb is a whole word too: "Address" is not an add request.
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Address the padding", &add_rule(), 2000.0).is_err());
        // "phase 15" does not authorize max_members 5.
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "phase 15 is done"));
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "phase 15 is done", &roster_rule(&["max", "5"]), 2000.0).is_err());
        // The right verb and whole member id work.
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Please remove dev from the roster"));
        verify_roster_in(&db, "claude/x/lead", "c1", "Please remove dev from the roster", &roster_rule(&["remove", "dev"]), 2000.0).unwrap();
        // U32: a plain "add" in either language is an add request.
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "может еще одного дипсик бэкендера добавим?"));
        verify_roster_in(&db, "claude/x/lead", "c1", "может еще одного дипсик бэкендера добавим?", &add_rule(), 2000.0).unwrap();
    }

    #[test]
    fn a_phase_addition_needs_the_request_then_a_newer_confirmation() {
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(1000.0, "Please add a task to phase 3: the hotfix"));
        add(&db, "claude/x/lead", "c1", prompt(1010.0, "Add a task to the running phase"));
        add(&db, "claude/x/lead", "c1", prompt(1020.0, "Add a task to phase 4"));
        add(&db, "claude/x/lead", "c1", prompt(1030.0, "Добавь задачу в текущую фазу"));
        let request = |text: &str, phase: u32| verify_phase_in(&db, "claude/x/lead", "c1", text, phase, PhaseWord::Request, 0.0, 1100.0);
        assert!(request("Please add a task to phase 3", 3).is_ok());
        assert!(request("Please add a task to phase 3", 4).is_err());   // names another phase
        assert!(request("Add a task to the running phase", 7).is_ok());  // "the running phase" names it
        assert!(request("Add a task to phase 4", 3).is_err());
        assert!(request("Добавь задачу в текущую фазу", 2).is_ok());
        // A confirmation must be newer than the warning and not name another phase.
        add(&db, "claude/x/lead", "c1", prompt(1050.0, "Yes, add it"));
        add(&db, "claude/x/lead", "c1", prompt(1060.0, "Confirm phase 5"));
        let confirm = |text: &str, phase: u32, after: f64| verify_phase_in(&db, "claude/x/lead", "c1", text, phase, PhaseWord::Confirm, after, 1100.0);
        assert!(confirm("Yes, add it", 3, 1040.0).is_ok());
        assert!(confirm("Yes, add it", 3, 1055.0).is_err());
        assert!(confirm("Confirm phase 5", 3, 1040.0).is_err());
        assert!(confirm("Confirm phase 5", 5, 1040.0).is_ok());
        // "address" is not "add".
        add(&db, "claude/x/lead", "c1", prompt(1070.0, "Address the task in phase 3"));
        assert!(request("Address the task in phase 3", 3).is_err());
    }

    #[test]
    fn a_roster_word_must_name_the_change() {
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Please add bob to the roster"));
        let needed = roster_rule(&["bob"]);
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Please add bob to the roster", &needed, 2010.0).is_ok());
        // A sentence naming another member is not this change.
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Please add bob to the roster", &roster_rule(&["alice"]), 2010.0).is_err());
        // The same 30-minute rule applies.
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Please add bob to the roster", &needed, 2000.0 + FRESH_SECONDS + 1.0).is_err());
    }

    #[test]
    fn an_add_word_names_the_member_by_role_or_agent_and_its_count() {
        let sentence = "может еще одного дипсик бэкендера добавим?";
        assert!(names_member(sentence, "Backend", "dsh", ""));
        assert!(names_member("maybe add one more DeepSeek backend?", "Backend", "dsh", ""));
        assert!(names_member("add another opus please", "Backend", "claude", "Opus"));
        assert!(names_member("add a codex reviewer", "Reviewer", "codex", ""));
        assert!(names_member("add an expert", "Expert", "claude", ""));
        // A sentence naming another role is not this member.
        assert!(!names_member("может еще одного фронтендера добавим?", "Backend", "dsh", ""));
        assert!(!names_member("add one more tester", "Backend", "dsh", ""));
        // The count is one unless a number is said; "one more" is one.
        assert_eq!(add_count(sentence), 1);
        assert_eq!(add_count("add a backend"), 1);
        assert_eq!(add_count("add two backends"), 2);
        assert_eq!(add_count("add 3 testers"), 3);
        assert_eq!(add_count("добавь двух бэкендеров"), 2);
        assert_eq!(add_count("добавь 5 тестеров"), 5);
        // U49: a scale names the role in the plural too.
        assert!(names_member("add two backends", "Backend", "codex", ""));
        assert!(names_member("add two testers", "Tester", "claude", ""));
    }

    #[test]
    fn a_count_may_be_said_as_a_number_word() {
        // U49: the word rule asks for the count; "two" names 2 as "2" does.
        let rule = WordRule { phrases: &[], all: vec!["2".into()], add: true };
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Please add two backend members to the roster"));
        verify_roster_in(&db, "claude/x/lead", "c1", "Please add two backend members to the roster", &rule, 2010.0).unwrap();
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Please add three backend members to the roster"));
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Please add three backend members to the roster", &rule, 2010.0).is_err());
        // A count written as a word does not authorize another count.
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Please add 2 backend members to the roster"));
        verify_roster_in(&db, "claude/x/lead", "c1", "Please add 2 backend members to the roster", &rule, 2010.0).unwrap();
    }
}
