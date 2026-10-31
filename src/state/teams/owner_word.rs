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

/// N15: the verbs an owner may say to remove a member, English and Russian.
/// An English verb is a whole word; a Russian one matches at a word start, so
/// "убери" covers "уберите" and "замени" covers "замените".
pub(crate) const REMOVE_VERBS: &[&str] = &[
    "remove", "removed", "removing", "delete", "deleted", "drop", "убери", "уберите", "убрать", "удали", "удалите", "удалить",
];

/// N15: the verbs an owner may say to swap a member, English and Russian; the
/// U26 example "put frontend on Opus" says "put".
pub(crate) const SWAP_VERBS: &[&str] = &[
    "swap", "swapped", "replace", "replaced", "switch", "put", "change", "move", "замени", "замените", "заменить",
    "смени", "смените", "поменяй", "поменяйте", "переведи", "перевести", "переключи",
];

/// N15: what the owner says when raising the member cap, English and Russian.
pub(crate) const MAX_VERBS: &[&str] = &["max", "maximum", "limit", "cap", "лимит", "максимум", "макс", "предел"];

/// U54: what the owner says to relaunch a member's session.
pub(crate) const RELAUNCH_VERBS: &[&str] = &["relaunch", "restart", "start", "запусти", "запустите", "перезапусти", "перезапустить"];

/// U49: the words that ask a member to drain, English and Russian.
pub(crate) const DRAIN_PHRASES: &[&str] = &["drain", "remove", "вывед", "убери", "убрать"];

/// U32: what the owner's sentence must contain for one roster change: one of
/// `phrases` (for an add, the verb "add"/"добав" instead), every token in
/// `all` (a number), one of `verbs`, and one of `names` (the member).
#[derive(Clone, Debug, Default)]
pub(crate) struct WordRule {
    pub phrases: &'static [&'static str],
    pub all: Vec<String>,
    pub add: bool,
    pub verbs: &'static [&'static str],
    pub names: Vec<String>,
    /// U56: a sentence that meets the rule, which a refusal names.
    pub expect: String,
}

/// U56: the sentence an approval expects, as a refusal names it: the plan's,
/// or phase `n`'s ("phase N approved" as well as the plan phrases).
pub(crate) fn approval_sentence(phase: Option<u32>) -> String {
    match phase {
        None => "\"plan approved\" (or \"план принят\")".to_owned(),
        Some(n) => format!("\"phase {n} approved\" (or \"фаза {n} принята\", \"next phase plan approved\")"),
    }
}

/// U56: phase `n` is approved by its own sentence too.
fn phase_phrases(n: u32) -> [String; 2] {
    [format!("phase {n} approved"), format!("фаза {n} принята")]
}

impl WordRule {
    /// Whether this change needs the owner's word at all.
    pub(crate) fn needed(&self) -> bool {
        self.add || !self.all.is_empty() || !self.verbs.is_empty() || !self.names.is_empty()
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

/// U32/N7: how many members the owner's sentence asks to add. The number is the
/// one said next to the add verb ("add two backends", "добавь двух бэкендеров"),
/// never one elsewhere in the sentence ("добавь бэкенда для фазы 3" is one);
/// a sentence that says no number is one. Bounded to the roster's own limit.
pub(crate) fn add_count(text: &str) -> usize {
    let lowered = text.to_lowercase();
    let words: Vec<&str> = lowered.split(|ch: char| !ch.is_alphanumeric() && ch != '_').filter(|word| !word.is_empty()).collect();
    let number = |word: &str| -> Option<usize> {
        if word.chars().all(|ch| ch.is_ascii_digit()) {
            word.parse::<usize>().ok()
        } else {
            NUMBER_WORDS.iter().find(|(written, _)| *written == word).map(|(_, number)| *number)
        }
    };
    for (index, word) in words.iter().enumerate() {
        if !(has_token(word, "add") || word.starts_with("добав")) {
            continue;
        }
        // The count sits right after the verb ("add two", "добавь ещё двух") or
        // right before it ("two more backends to add").
        for offset in [1usize, 2] {
            if let Some(found) = words.get(index + offset).and_then(|word| number(word)) {
                return found.clamp(1, 32);
            }
        }
        for offset in [1usize, 2] {
            if let Some(found) = index.checked_sub(offset).and_then(|at| words.get(at)).and_then(|word| number(word)) {
                return found.clamp(1, 32);
            }
        }
    }
    1
}

/// U32/N8: whether the owner's sentence names the member being added, by its
/// role or its agent/model, as the owner says them (never by the generated id).
/// A sentence that names a role names *this* member's role: "a DeepSeek
/// frontender" is not a Backend. An agent or model word alone is enough only
/// when no other role is named.
pub(crate) fn names_member(text: &str, role: &str, agent: &str, model: &str) -> bool {
    let lowered = text.to_lowercase();
    let named: Vec<usize> = ROLE_ALIASES
        .iter()
        .enumerate()
        .filter(|(_, (_, aliases))| aliases.iter().any(|alias| alias_in(&lowered, alias)))
        .map(|(index, _)| index)
        .collect();
    let mine = role_group(role);
    if !named.is_empty() && !mine.is_some_and(|mine| named.contains(&mine)) {
        return false;
    }
    let role_named = mine.is_some_and(|mine| named.contains(&mine));
    let agent_named = agent_aliases(agent)
        .iter()
        .chain(model_aliases(model).iter())
        .any(|alias| alias_in(&lowered, alias));
    role_named || agent_named
}

/// Which role group a role name belongs to, if it is one the owner knows.
fn role_group(role: &str) -> Option<usize> {
    let role = role.to_lowercase();
    ROLE_ALIASES.iter().position(|(keys, _)| keys.iter().any(|key| role.contains(key)))
}

/// U32/N15: the aliases the owner may say for a member's role, so a roster
/// change can name the member by role instead of its generated id.
pub(crate) fn role_aliases(role: &str) -> &'static [&'static str] {
    role_group(role).map(|index| ROLE_ALIASES[index].1).unwrap_or(&[])
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

/// N8: an alias matches only at a word start, never inside another word
/// ("кими" is not "такими"); what follows is its inflection or plural
/// ("бэкендера", "backends").
fn alias_in(lowered: &str, alias: &str) -> bool {
    if alias.is_empty() {
        return true;
    }
    let mut from = 0;
    while let Some(at) = lowered[from..].find(alias) {
        let start = from + at;
        let before = lowered[..start].chars().next_back();
        if before.is_none_or(|ch| !ch.is_alphanumeric() && ch != '-' && ch != '_') {
            return true;
        }
        from = start + alias.len();
    }
    false
}

/// N15: a verb is said whole in English and at a word start in Russian, so
/// "remove" is not "removed" but "убери" is "уберите".
fn verb_in(lowered: &str, verb: &str) -> bool {
    if verb.is_ascii() {
        has_token(lowered, verb)
    } else {
        alias_in(lowered, verb)
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
/// meet `rule`: the roster phrase (or, for an add, the add verb), the
/// operation's own verb and the member it names.
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
    let expect = if rule.expect.is_empty() { "the roster phrase and the change it names".to_owned() } else { format!("\"{}\"", rule.expect) };
    ScanOptions { phrases: rule.phrases, needed: &rule.all, exact, add: rule.add, verbs: rule.verbs, names: &rule.names, accept: None, after: 0.0, expect }
}

/// Checks the lead's `owner_word` request against the lead session's events
/// and returns the quote to store: a plan approval, or phase `phase`'s.
pub(crate) fn verify(team: &Team, input: &Value, phase: Option<u32>, now: f64) -> anyhow::Result<OwnerWord> {
    let text = input["text"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .with_context(|| format!("owner_word.text is required: quote the owner's sentence, which says {}", approval_sentence(phase)))?;
    let lead = team.member(&team.lead).context("The team has no lead")?;
    let session = lead.session.as_ref().context("The lead has no session to check the owner's word in")?;
    let db = super::super::journal::event_db().map_err(|error| anyhow::anyhow!("The session events are unavailable: {error}"))?;
    verify_approval_in(&db, &session.name, &session.conversation_id, text, phase, now)
}

/// The verification against one events database, so it is tested with a
/// fixture instead of the live state.
#[cfg(test)]
pub(crate) fn verify_in(db: &Connection, name: &str, conversation: &str, text: &str, now: f64) -> anyhow::Result<OwnerWord> {
    verify_approval_in(db, name, conversation, text, None, now)
}

/// U56: a phase approval also takes "phase N approved" for its own phase,
/// and a refusal names the sentence it expects.
pub(crate) fn verify_approval_in(db: &Connection, name: &str, conversation: &str, text: &str, phase: Option<u32>, now: f64) -> anyhow::Result<OwnerWord> {
    let own = phase.map(phase_phrases);
    let mut phrases: Vec<&str> = KEY_PHRASES.to_vec();
    phrases.extend(own.iter().flatten().map(String::as_str));
    let expect = approval_sentence(phase);
    verify_scan(db, name, conversation, text, now, ScanOptions { phrases: &phrases, needed: &[], exact: false, add: false, verbs: &[], names: &[], accept: None, after: 0.0, expect })
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
    // C1: a refusal never asks: "не добавляй задачу в текущую фазу" is not the
    // owner asking for an addition.
    if negated(lowered) {
        return false;
    }
    let adds = has_token(lowered, "add") || lowered.contains("добав");
    let task = lowered.contains("task") || lowered.contains("задач");
    let current = ["current phase", "running phase", "this phase", "текущ", "идущ", "эту фаз", "этой фаз"]
        .iter()
        .any(|phrase| lowered.contains(phrase));
    let names = named_phases(lowered);
    adds && task && (names.contains(&phase) || (names.is_empty() && current))
}

/// C1: a confirmation is a short affirmative sentence, never a refusal and
/// never an unrelated sentence that happens to say "ok". The longest one may
/// be; a paragraph is not an answer to the warning.
const CONFIRM_CHARS: usize = 160;

/// C1: words that make a sentence a refusal in either language.
fn negated(lowered: &str) -> bool {
    const NEGATIONS: &[&str] = &[
        "no", "not", "never", "dont", "doesnt", "didnt", "wont", "wouldnt", "cant", "cannot", "couldnt", "shouldnt",
        "stop", "cancel", "не", "нет", "ни", "нельзя", "никогда", "отмена", "отмени", "хватит", "стоп",
    ];
    let words: Vec<&str> = lowered.split(|ch: char| !ch.is_alphanumeric()).filter(|word| !word.is_empty()).collect();
    if words.iter().any(|word| NEGATIONS.contains(word)) {
        return true;
    }
    // The tokenizer splits an apostrophe off ("don't" is "don" and "t").
    ["don't", "doesn't", "didn't", "won't", "wouldn't", "can't", "couldn't", "shouldn't"]
        .iter()
        .any(|form| lowered.contains(form))
}

/// C1/N2: the words a confirmation may add after its first one: the addition
/// itself, politeness, and the phase this warning is about.
const CONFIRM_WORDS: &[&str] = &[
    "add", "confirm", "confirmed", "it", "now", "please", "the", "task", "to", "in", "current", "running", "this",
    "да", "добавь", "подтверждаю", "ок", "ok", "okay", "окей", "в", "на", "текущую", "текущей", "эту", "этой", "идущ",
];

/// C1/N2: the rest of a confirmation may only name the addition, politeness and
/// the phase itself; "add it to the next phase", "after the review", "later" or
/// another number are not confirmations of this warning.
fn confirmation_rest(words: &[&str], phase: u32) -> bool {
    let number = phase.to_string();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if word == "phase" || word.starts_with("фаз") {
            if words.get(i + 1).is_some_and(|next| *next == number.as_str()) {
                i += 2;
                continue;
            }
            if i > 0 && matches!(words[i - 1], "current" | "running" | "this" | "текущую" | "текущей" | "эту" | "этой" | "идущ") {
                i += 1;
                continue;
            }
            return false;
        }
        if word == number.as_str() || CONFIRM_WORDS.contains(&word) {
            i += 1;
            continue;
        }
        return false;
    }
    true
}

/// C1: a confirmation is a bare yes/да/ok/confirm/add, or a short sentence that
/// starts with one and names only the addition. "No, don't add it" and "не
/// добавляй" are refusals, and "ok, what's the status?" is not an answer.
fn confirms(lowered: &str, phase: u32) -> bool {
    let text = lowered.trim().trim_matches(|ch: char| ch.is_whitespace() || ".,!?;:\"'()[]{}…".contains(ch));
    if text.is_empty() || text.chars().count() > CONFIRM_CHARS || negated(text) {
        return false;
    }
    // A confirmation naming another phase is not this one's.
    if named_phases(text).iter().any(|n| *n != phase) {
        return false;
    }
    let add = has_token(text, "add") || text.contains("добав");
    let confirm = has_token(text, "confirm") || has_token(text, "confirmed") || text.contains("подтвер");
    let bare = matches!(
        text,
        "yes" | "да" | "ok" | "okay" | "ок" | "окей" | "sure" | "confirm" | "confirmed" | "add" | "add it" | "добавь"
            | "подтверждаю"
    );
    if bare {
        return true;
    }
    let words: Vec<&str> = text.split(|ch: char| !ch.is_alphanumeric()).filter(|word| !word.is_empty()).collect();
    let Some(first) = words.first().copied() else { return false };
    let prefix = matches!(
        first,
        "yes" | "да" | "sure" | "confirm" | "confirmed" | "подтверждаю" | "add" | "добавь" | "ok" | "okay" | "ок" | "окей"
    );
    if !prefix {
        return false;
    }
    // "ok"/"ок" alone is an answer only with the addition named.
    if matches!(first, "ok" | "okay" | "ок" | "окей") && !(add || confirm) {
        return false;
    }
    confirmation_rest(&words[1..], phase)
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
    // C1: a confirmation is the owner's whole sentence, never a fragment of a
    // longer one; a request may be quoted from a sentence that also explains it.
    let exact = matches!(kind, PhaseWord::Confirm);
    let expect = phase_word_sentence(phase, kind);
    verify_scan(db, name, conversation, text, now, ScanOptions { phrases: &[], needed: &[], exact, add: false, verbs: &[], names: &[], accept: Some(&accept), after, expect: expect.clone() })
        .with_context(|| match kind {
            PhaseWord::Request => format!("The owner has not asked to add a task to phase {phase}: quote the owner's sentence that says {expect}"),
            PhaseWord::Confirm => format!("The owner has not confirmed the addition to phase {phase} after the warning: quote the owner's newer sentence {expect}"),
        })
}

/// U56: the sentence an addition to a running phase expects.
fn phase_word_sentence(phase: u32, kind: PhaseWord) -> String {
    match kind {
        PhaseWord::Request => format!("\"add a task to phase {phase}\" (or \"to the current phase\")"),
        PhaseWord::Confirm => "\"yes, add it\" (or \"да, добавь\")".to_owned(),
    }
}

/// U39/U42: what the owner says to change an accepted phase on the lead's
/// request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhaseChange {
    Discard,
    ToDraft,
    /// U63 (owner, 2026-10-31): an edit, an addition or a move of an
    /// accepted phase.
    Edit,
    /// U63: removing the lead's own pause of the phase.
    Unpause,
    /// U63: whether the owner accepts the phase.
    Acceptance,
    /// U63: the owner's acceptance of the phase itself.
    Accept,
}

/// The verbs of a discard, English whole and Russian at a word start.
const DISCARD_VERBS: &[&str] = &["discard", "drop", "remove", "delete", "scrap", "cancel", "отмен", "удали", "убери", "выброс", "выкин"];
/// The words of a return to a draft.
const DRAFT_WORDS: &[&str] = &["draft", "drafts", "redraft", "черновик"];
/// U63: the verbs of an edit, an addition or a move of a phase.
const EDIT_VERBS: &[&str] = &[
    "edit", "change", "rename", "move", "add", "reorder", "update", "replace", "remove", "rework",
    "измен", "поменя", "перенес", "передвин", "добав", "редакт", "переимен", "передела", "убери", "удали",
];
/// U63: the verbs that end a pause by themselves.
const UNPAUSE_VERBS: &[&str] = &["unpause", "resume", "continue", "unblock", "возобнов", "продолж", "сними паузу", "снять паузу", "сними с паузы"];
/// U63: the words of who accepts a phase.
const ACCEPTANCE_WORDS: &[&str] = &["acceptance", "приемк", "приёмк"];
/// U63: the verbs of the owner's acceptance.
const ACCEPT_VERBS: &[&str] = &["accept", "accepted", "прими", "принима", "принят"];

/// The owner's sentence names phase `phase` and the change; a refusal never
/// authorizes a change (#595 C1).
fn asks_phase_change(lowered: &str, phase: u32, change: PhaseChange) -> bool {
    if negated(lowered) {
        return false;
    }
    let words = match change {
        PhaseChange::Discard => DISCARD_VERBS,
        PhaseChange::ToDraft => DRAFT_WORDS,
        PhaseChange::Edit => EDIT_VERBS,
        PhaseChange::Unpause => {
            // "remove the pause of phase 3" says the change too.
            let removes = (verb_in(lowered, "remove") || verb_in(lowered, "lift") || verb_in(lowered, "убери"))
                && (lowered.contains("pause") || lowered.contains("пауз"));
            return named_phases(lowered).contains(&phase) && (removes || UNPAUSE_VERBS.iter().any(|word| verb_in(lowered, word)));
        }
        PhaseChange::Acceptance => ACCEPTANCE_WORDS,
        PhaseChange::Accept => {
            // The acceptance itself, not a sentence about who accepts.
            if ACCEPTANCE_WORDS.iter().any(|word| verb_in(lowered, word)) {
                return false;
            }
            ACCEPT_VERBS
        }
    };
    named_phases(lowered).contains(&phase) && words.iter().any(|word| verb_in(lowered, word))
}

/// U39/U42: verifies the owner's word for discarding phase `phase`, or for
/// returning it to a draft: a fresh sentence of the lead's own session that
/// names the phase and the change.
pub(crate) fn verify_phase_change(team: &Team, input: &Value, phase: u32, change: PhaseChange, now: f64) -> anyhow::Result<OwnerWord> {
    let text = input["text"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .context("owner_word.text is required: quote the owner's sentence")?;
    let lead = team.member(&team.lead).context("The team has no lead")?;
    let session = lead.session.as_ref().context("The lead has no session to check the owner's word in")?;
    let db = super::super::journal::event_db().map_err(|error| anyhow::anyhow!("The session events are unavailable: {error}"))?;
    verify_phase_change_in(&db, &session.name, &session.conversation_id, text, phase, change, now)
}

/// The fixture-testable phase change check. #595 C1: the quote is the owner's
/// whole sentence, never a fragment of one that also refuses it.
pub(crate) fn verify_phase_change_in(db: &Connection, name: &str, conversation: &str, text: &str, phase: u32, change: PhaseChange, now: f64) -> anyhow::Result<OwnerWord> {
    let accept = move |lowered: &str| asks_phase_change(lowered, phase, change);
    let expect = match change {
        PhaseChange::Discard => format!("\"discard phase {phase}\""),
        PhaseChange::ToDraft => format!("\"return phase {phase} to a draft\""),
        PhaseChange::Edit => format!("\"change phase {phase}: ...\""),
        PhaseChange::Unpause => format!("\"remove the pause of phase {phase}\""),
        PhaseChange::Acceptance => format!("\"phase {phase} needs my acceptance\""),
        PhaseChange::Accept => format!("\"phase {phase} accepted\""),
    };
    verify_scan(db, name, conversation, text, now, ScanOptions { phrases: &[], needed: &[], exact: true, add: false, verbs: &[], names: &[], accept: Some(&accept), after: 0.0, expect: expect.clone() })
        .with_context(|| match change {
            PhaseChange::Discard => format!("The owner has not asked to discard phase {phase}: quote the owner's whole sentence, such as {expect}"),
            PhaseChange::ToDraft => format!("The owner has not asked to return phase {phase} to a draft: quote the owner's whole sentence, such as {expect}"),
            PhaseChange::Edit => format!("The owner has not asked to change phase {phase}: quote the owner's whole sentence, such as {expect}"),
            PhaseChange::Unpause => format!("The owner has not asked to remove the pause of phase {phase}: quote the owner's whole sentence, such as {expect}"),
            PhaseChange::Acceptance => format!("The owner has not said who accepts phase {phase}: quote the owner's whole sentence, such as {expect}"),
            PhaseChange::Accept => format!("The owner has not accepted phase {phase}: quote the owner's whole sentence, such as {expect}"),
        })
}

struct ScanOptions<'a> {
    phrases: &'a [&'a str],
    needed: &'a [String],
    exact: bool,
    /// U32: an add request, which says "add"/"добав" instead of a roster phrase.
    add: bool,
    /// N15: one of these verbs must be said (English whole, Russian at a start).
    verbs: &'a [&'a str],
    /// N15: one of these must name the member (its id or a role alias).
    names: &'a [String],
    /// U29: a check of the lowered sentence instead of the phrases.
    accept: Option<&'a dyn Fn(&str) -> bool>,
    /// U29: the sentence must be newer than this time.
    after: f64,
    /// U56: the sentence the refusal names.
    expect: String,
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
        // #595 N1/C1: a refusal never authorizes any owner word, whatever the
        // rule: "Don't remove backend2" and "не отменяй фазу 3" are not words.
        if negated(&lowered) {
            continue;
        }
        let said = if let Some(accept) = options.accept {
            accept(&lowered)
        } else if options.add {
            // U32: the owner asks to add someone, in either language; "add" is
            // a whole token, so "address" is not an add.
            has_token(&lowered, "add") || lowered.contains("добав")
        } else {
            // N15: the operation's verb in either language, one name of the
            // member the change is about, and - when the rule has no verb of
            // its own - the roster phrase. A verb alone is a roster change, as
            // "add"/"добав" alone is for an add.
            let phrase = !options.verbs.is_empty() || options.phrases.iter().any(|phrase| lowered.contains(phrase));
            let verb = options.verbs.is_empty() || options.verbs.iter().any(|verb| verb_in(&lowered, verb));
            let named = options.names.is_empty()
                || options.names.iter().any(|name| {
                    let name = name.to_lowercase();
                    if name.is_ascii() { has_token(&lowered, &name) } else { alias_in(&lowered, &name) }
                });
            phrase && verb && named
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
    // U56: the refusal names the sentence this operation expects.
    anyhow::bail!(
        "The owner's word is not in the lead session's last 30 minutes: quote the owner's own sentence that says {}",
        options.expect
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

    /// U56: a refused word names the sentence its operation expects, and a
    /// phase approval takes "phase N approved" for its own phase only.
    #[test]
    fn a_refused_word_names_the_expected_sentence() {
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "please continue"));
        let error = verify_in(&db, "claude/x/lead", "c1", "please continue", 2000.0).unwrap_err().to_string();
        assert!(error.contains("\"plan approved\" (or \"план принят\")"), "{error}");
        assert!(!error.contains("roster"), "{error}");
        let error = verify_approval_in(&db, "claude/x/lead", "c1", "please continue", Some(3), 2000.0).unwrap_err().to_string();
        assert!(error.contains("\"phase 3 approved\""), "{error}");
        // The phase's own sentence approves that phase, in either language.
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Phase 3 approved, go"));
        add(&db, "claude/x/lead", "c1", prompt(2001.0, "фаза 4 принята"));
        assert!(verify_approval_in(&db, "claude/x/lead", "c1", "Phase 3 approved", Some(3), 2010.0).is_ok());
        assert!(verify_approval_in(&db, "claude/x/lead", "c1", "фаза 4 принята", Some(4), 2010.0).is_ok());
        // Another phase's sentence, a plan approval and a refusal do not.
        assert!(verify_approval_in(&db, "claude/x/lead", "c1", "Phase 3 approved", Some(4), 2010.0).is_err());
        assert!(verify_approval_in(&db, "claude/x/lead", "c1", "Phase 3 approved", None, 2010.0).is_err());
        add(&db, "claude/x/lead", "c1", prompt(2002.0, "phase 5 approved? no, not yet"));
        assert!(verify_approval_in(&db, "claude/x/lead", "c1", "phase 5 approved", Some(5), 2010.0).is_err());
        // A roster rule names its own sentence; phase changes and additions too.
        let rule = WordRule { phrases: ROSTER_PHRASES, verbs: REMOVE_VERBS, names: vec!["dev".into()], expect: "remove dev".into(), ..Default::default() };
        let error = verify_roster_in(&db, "claude/x/lead", "c1", "please continue", &rule, 2010.0).unwrap_err().to_string();
        assert!(error.contains("\"remove dev\""), "{error}");
        let error = verify_phase_change_in(&db, "claude/x/lead", "c1", "please continue", 2, PhaseChange::Discard, 2010.0).unwrap_err().to_string();
        assert!(error.contains("\"discard phase 2\""), "{error}");
        let error = verify_phase_change_in(&db, "claude/x/lead", "c1", "please continue", 2, PhaseChange::Accept, 2010.0).unwrap_err().to_string();
        assert!(error.contains("\"phase 2 accepted\""), "{error}");
        let error = verify_phase_in(&db, "claude/x/lead", "c1", "please continue", 2, PhaseWord::Request, 0.0, 2010.0).unwrap_err().to_string();
        assert!(error.contains("\"add a task to phase 2\""), "{error}");
    }

    /// U63 (owner, 2026-10-31): the sentences that let the lead change an
    /// accepted phase, remove its own pause, set who accepts a phase and
    /// accept one name the phase and the change; another change does not.
    #[test]
    fn the_owners_phase_control_words_name_the_phase_and_the_change() {
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Move phase 3 before phase 2"));
        add(&db, "claude/x/lead", "c1", prompt(2001.0, "Remove the pause of phase 3"));
        add(&db, "claude/x/lead", "c1", prompt(2002.0, "Pause phase 4"));
        add(&db, "claude/x/lead", "c1", prompt(2003.0, "Phase 3 needs my acceptance"));
        add(&db, "claude/x/lead", "c1", prompt(2004.0, "Phase 3 accepted"));
        add(&db, "claude/x/lead", "c1", prompt(2005.0, "сними паузу с фазы 5"));
        let check = |text: &str, phase: u32, change: PhaseChange| verify_phase_change_in(&db, "claude/x/lead", "c1", text, phase, change, 2010.0).is_ok();
        assert!(check("Move phase 3 before phase 2", 3, PhaseChange::Edit));
        assert!(check("Remove the pause of phase 3", 3, PhaseChange::Unpause));
        assert!(check("сними паузу с фазы 5", 5, PhaseChange::Unpause));
        assert!(!check("Pause phase 4", 4, PhaseChange::Unpause));
        assert!(check("Phase 3 needs my acceptance", 3, PhaseChange::Acceptance));
        assert!(!check("Phase 3 needs my acceptance", 3, PhaseChange::Accept));
        assert!(check("Phase 3 accepted", 3, PhaseChange::Accept));
        assert!(!check("Phase 3 accepted", 4, PhaseChange::Accept));
        assert!(!check("Phase 3 accepted", 3, PhaseChange::Edit));
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
        assert!(verify(&team, &serde_json::json!({"text":"plan approved"}), None, 2000.0).is_err());
    }

    fn roster_rule(all: &[&str]) -> WordRule {
        WordRule { phrases: ROSTER_PHRASES, all: all.iter().map(|value| (*value).to_owned()).collect(), add: false, ..Default::default() }
    }

    fn add_rule() -> WordRule {
        WordRule { phrases: &[], all: Vec::new(), add: true, ..Default::default() }
    }

    /// N15: a removal or a swap the way the owner says it: the verb in either
    /// language, and the member by its id or its role.
    fn member_rule(verbs: &'static [&'static str], id: &str, role: &str) -> WordRule {
        let mut names = vec![id.to_owned()];
        names.extend(role_aliases(role).iter().map(|alias| (*alias).to_owned()));
        WordRule { phrases: ROSTER_PHRASES, all: Vec::new(), add: false, verbs, names, expect: String::new() }
    }

    /// N15: the cap change, naming the number the owner said.
    fn max_rule(cap: u64) -> WordRule {
        WordRule { phrases: ROSTER_PHRASES, all: vec![cap.to_string()], add: false, verbs: MAX_VERBS, names: Vec::new(), expect: String::new() }
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

    /// C1 (#511): a refusal never confirms or asks, an unrelated sentence with
    /// "ok" is not the answer to the warning, and the quote is the whole
    /// sentence, never a fragment of it.
    #[test]
    fn a_refusal_never_confirms_a_phase_addition() {
        let db = database();
        for (at, said) in [
            (2000.0, "Please add a task to phase 3: the hotfix"),
            (2010.0, "Yes, add it"),
            (2020.0, "Confirmed, add it now"),
            (2030.0, "да, добавь"),
            (2040.0, "подтверждаю"),
            (2045.0, "ок, добавь"),
            (2050.0, "No, don't add it"),
            (2060.0, "не добавляй"),
            (2070.0, "ok, what's the status"),
            (2080.0, "No, don't add a task to phase 3"),
            (2090.0, "не добавляй задачу в текущую фазу"),
        ] {
            add(&db, "claude/x/lead", "c1", prompt(at, said));
        }
        let request = |text: &str| verify_phase_in(&db, "claude/x/lead", "c1", text, 3, PhaseWord::Request, 0.0, 2100.0);
        let confirm = |text: &str| verify_phase_in(&db, "claude/x/lead", "c1", text, 3, PhaseWord::Confirm, 1990.0, 2100.0);
        // The real confirmations, in both languages, still work.
        for said in ["Yes, add it", "Confirmed, add it now", "да, добавь", "подтверждаю", "ок, добавь"] {
            confirm(said).unwrap_or_else(|error| panic!("{said}: {error}"));
        }
        // A refusal never confirms.
        for said in ["No, don't add it", "не добавляй"] {
            let error = confirm(said).unwrap_err().to_string();
            assert!(error.contains("has not confirmed"), "{said}: {error}");
        }
        // An unrelated sentence with "ok" in it is not the answer.
        let error = confirm("ok, what's the status").unwrap_err().to_string();
        assert!(error.contains("has not confirmed"), "{error}");
        // The quote is the whole sentence: a fragment is not the owner's word.
        assert!(confirm("add it").is_err());
        assert!(confirm("Yes, add it to phase 3").is_err());
        // A request is never a refusal.
        request("Please add a task to phase 3").unwrap();
        for said in ["No, don't add a task to phase 3", "не добавляй задачу в текущую фазу"] {
            let error = request(said).unwrap_err().to_string();
            assert!(error.contains("has not asked"), "{said}: {error}");
        }
    }

    /// #595 N2: a deferral or a redirection is not a confirmation, even when it
    /// starts with yes/sure/add; the phase this warning is about may be named.
    #[test]
    fn a_deferral_never_confirms_a_phase_addition() {
        let db = database();
        for (at, said) in [
            (2000.0, "Please add a task to phase 3"),
            (2010.0, "Add it to the next phase instead"),
            (2020.0, "Yes, but after the review"),
            (2030.0, "sure, later"),
            (2040.0, "yes, add it now"),
            (2050.0, "yes, add it to phase 3"),
            (2060.0, "да, добавь в текущую фазу"),
        ] {
            add(&db, "claude/x/lead", "c1", prompt(at, said));
        }
        let confirm = |text: &str| verify_phase_in(&db, "claude/x/lead", "c1", text, 3, PhaseWord::Confirm, 1990.0, 2100.0);
        for said in ["Add it to the next phase instead", "Yes, but after the review", "sure, later"] {
            let error = confirm(said).unwrap_err().to_string();
            assert!(error.contains("has not confirmed"), "{said}: {error}");
        }
        for said in ["yes, add it now", "yes, add it to phase 3", "да, добавь в текущую фазу"] {
            confirm(said).unwrap_or_else(|error| panic!("{said}: {error}"));
        }
    }

    /// #595 N1: a refusal never authorizes a roster, scale or drain word in any
    /// language, while the plain sentence still does.
    #[test]
    fn a_refusal_never_authorizes_a_roster_word() {
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "Don't remove backend2"));
        add(&db, "claude/x/lead", "c1", prompt(2010.0, "Please remove backend2"));
        let rule = member_rule(&["remove"], "backend2", "Backend");
        let error = verify_roster_in(&db, "claude/x/lead", "c1", "Don't remove backend2", &rule, 2020.0).unwrap_err().to_string();
        assert!(error.contains("owner's word"), "{error}");
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "Please remove backend2", &rule, 2020.0).is_ok());
        let db = database();
        add(&db, "claude/x/lead", "c1", prompt(2000.0, "не надо добавлять второго тестера"));
        let error = verify_roster_in(&db, "claude/x/lead", "c1", "не надо добавлять второго тестера", &add_rule(), 2010.0).unwrap_err().to_string();
        assert!(error.contains("owner's word"), "{error}");
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

    /// N15: the owner's own words for remove, swap and the cap authorize them,
    /// in English and Russian, naming the member by its id or its role.
    #[test]
    fn a_roster_word_speaks_both_languages_and_names_the_member_by_role() {
        let db = database();
        for (at, said) in [
            (2000.0, "убери dev из состава"),
            (2010.0, "замени dev на кими"),
            (2020.0, "put frontend on Opus"),
            (2030.0, "raise the limit to 8"),
            (2040.0, "уберите тестера из состава"),
        ] {
            add(&db, "claude/x/lead", "c1", prompt(at, said));
        }
        // "убери dev из состава": the Russian verb and the literal id.
        verify_roster_in(&db, "claude/x/lead", "c1", "убери dev из состава", &member_rule(REMOVE_VERBS, "dev", "Backend"), 2100.0).unwrap();
        // "уберите тестера из состава": the verb's plural and the role alias.
        verify_roster_in(&db, "claude/x/lead", "c1", "уберите тестера из состава", &member_rule(REMOVE_VERBS, "qa", "Tester"), 2100.0).unwrap();
        // "замени dev на кими" and the U26 "put frontend on Opus" are swaps.
        verify_roster_in(&db, "claude/x/lead", "c1", "замени dev на кими", &member_rule(SWAP_VERBS, "dev", "Backend"), 2100.0).unwrap();
        verify_roster_in(&db, "claude/x/lead", "c1", "put frontend on Opus", &member_rule(SWAP_VERBS, "frontend", "Frontend"), 2100.0).unwrap();
        // "raise the limit to 8" raises the cap the owner named.
        verify_roster_in(&db, "claude/x/lead", "c1", "raise the limit to 8", &max_rule(8), 2100.0).unwrap();
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "raise the limit to 8", &max_rule(9), 2100.0).is_err());
        // A removal that names another member or the wrong verb is not this change.
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "убери dev из состава", &member_rule(REMOVE_VERBS, "qa", "Tester"), 2100.0).is_err());
        assert!(verify_roster_in(&db, "claude/x/lead", "c1", "убери dev из состава", &member_rule(SWAP_VERBS, "dev", "Backend"), 2100.0).is_err());
    }

    /// N8: a sentence that names a role names this member's role; an agent or
    /// model word cannot stand for another role. Aliases match at a word start,
    /// so "кими" is not "такими", and English plurals count ("backends").
    #[test]
    fn an_add_word_names_this_members_role_and_no_fragment_of_another_word() {
        // "add one more DeepSeek frontender" is not a Backend, whatever its agent.
        assert!(!names_member("добавь ещё одного дипсик фронтендера", "Backend", "dsh", ""));
        assert!(!names_member("add one more DeepSeek frontender", "Backend", "dsh", ""));
        // "с такими же правами" contains "кими", but not at a word start.
        assert!(!names_member("добавь ещё одного бэкендера с такими же правами", "Ops", "kimi", ""));
        assert!(!names_member("add a member with the same rights", "Ops", "kimi", ""));
        // A plural names the role.
        assert!(names_member("add two backends", "Backend", "dsh", ""));
        assert!(names_member("add three testers", "Tester", "kimi", ""));
        assert!(names_member("add another opus please", "Backend", "claude", "Opus"));
        // The role's own alias still matches its inflections.
        assert!(names_member("добавь ещё одного бэкендера", "Backend", "dsh", ""));
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
        // The count is the number next to the add verb, never one elsewhere in
        // the sentence; "one more" is one.
        assert_eq!(add_count(sentence), 1);
        assert_eq!(add_count("add a backend"), 1);
        assert_eq!(add_count("add two backends"), 2);
        assert_eq!(add_count("add 3 testers"), 3);
        assert_eq!(add_count("добавь двух бэкендеров"), 2);
        assert_eq!(add_count("добавь 5 тестеров"), 5);
        assert_eq!(add_count("добавь еще двух бэкендеров"), 2);
        // "добавь бэкенда для фазы 3" is one add: 3 is the phase, not a count.
        assert_eq!(add_count("добавь бэкенда для фазы 3"), 1);
        assert_eq!(add_count("add a backend for phase 3"), 1);
        // U49: a scale names the role in the plural too.
        assert!(names_member("add two backends", "Backend", "codex", ""));
        assert!(names_member("add two testers", "Tester", "claude", ""));
    }

    #[test]
    fn a_count_may_be_said_as_a_number_word() {
        // U49: the word rule asks for the count; "two" names 2 as "2" does.
        let rule = WordRule { phrases: &[], all: vec!["2".into()], add: true, ..Default::default() };
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
