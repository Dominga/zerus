//! Attention notifications as the Qt client decides them: a port of
//! `AttentionTracker` (`tray/src/AttentionTracker.cpp`), of the shared
//! preferences check (`tray/src/SessionNotifications.h`), of the macOS
//! notifier's delivery state (`tray/src/MacAttentionNotifier.mm`) and of the
//! click target (`TrayAgent.cpp:166-193`, `:593-608`). The state keeps Qt's
//! JSON shape, so Qt's saved `attention/observed` can seed it.
//!
//! Pure: no I/O and no clock. The caller passes the time, the focus and the
//! notice IDs in, and reads rows the menu bar's marks overlay has applied
//! this device's marks to (`menubar::overlay`), as Qt's tracker reads
//! `FleetState`'s rows.
use crate::menubar::{Computer, Desk};
use crate::{ComputerId, ViewRow};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

pub const NEEDS_ATTENTION: &str = "Zerus \u{2014} Needs attention";
pub const NEW_REPLY: &str = "Zerus \u{2014} New reply";
pub const OPTIONAL_QUESTION: &str = "Zerus \u{2014} Optional question";
/// Question IDs remembered per conversation (`AttentionTracker.cpp:28`).
pub const SEEN_QUESTIONS: usize = 64;
/// A failed delivery is retried this many seconds later (`AttentionTracker.cpp:262`).
pub const RETRY_SECONDS: i64 = 60;
/// How long a click waits for its session (`TrayAgent.cpp:192`).
pub const CLICK_WAIT: Duration = Duration::from_secs(16);
/// Without permission (`MacAttentionNotifier.mm:90`).
pub const DENIED: &str = "Allow Zerus notifications in System Settings \u{2192} Notifications.";

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> &'a str {
    object.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `QJsonValue::toInteger()`: an integer, or any whole double within the
/// qint64 range; anything else is 0.
fn integer(object: &Map<String, Value>, key: &str) -> i64 {
    let Some(value) = object.get(key) else {
        return 0;
    };
    value
        .as_i64()
        .or_else(|| {
            value
                .as_f64()
                .filter(|number| {
                    // [-2^63, 2^63): exactly the doubles an i64 holds.
                    number.fract() == 0.0
                        && *number >= i64::MIN as f64
                        && *number < -(i64::MIN as f64)
                })
                .map(|number| number as i64)
        })
        .unwrap_or(0)
}

fn nonempty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.is_empty())
}

fn positive(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

/// A double as `QJsonValue(double)` keeps it: a whole number within 2^53 is
/// an integer (so the state reads like Qt's), any other a double.
fn qt_value(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() <= 9.007_199_254_740_992e15 {
        Value::from(value as i64)
    } else {
        json!(value)
    }
}

/// `concise`: whitespace runs become one space, control characters a space,
/// and text over `limit` UTF-16 units (Qt's `QString::size`) ends with an
/// ellipsis (`AttentionTracker.cpp:18-24`). A cut never splits a character.
pub fn concise(value: &str, limit: usize) -> String {
    let simplified = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned: String = simplified
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if cleaned.encode_utf16().count() <= limit {
        return cleaned;
    }
    let mut cut = String::new();
    let mut units = 0;
    for c in cleaned.chars() {
        units += c.len_utf16();
        if units > limit.saturating_sub(1) {
            break;
        }
        cut.push(c);
    }
    cut.push('\u{2026}');
    cut
}

/// A double as Qt's compact JSON writes it: a whole number within 2^53 as
/// an integer, else the shortest form.
fn qt_number(value: f64) -> String {
    qt_value(value).to_string()
}

/// `notificationRequest`: `[request, started]`, the start time only for an
/// identity-less request (`AttentionTracker.cpp:39-42`).
pub fn request_json(request: &str, started: Option<f64>) -> String {
    let started = match started.and_then(positive) {
        Some(at) if request.is_empty() => qt_number(at),
        _ => "null".to_owned(),
    };
    format!("[{},{started}]", Value::from(request))
}

/// `QByteArray::toBase64(Base64UrlEncoding | OmitTrailingEquals)`.
pub fn base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// `QByteArray::fromBase64(Base64UrlEncoding)`: the URL alphabet, with or
/// without trailing `=`.
pub fn from_base64url(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text.trim_end_matches('=')).ok()
}

pub fn encode_token(target: &Map<String, Value>) -> String {
    base64url(Value::Object(target.clone()).to_string().as_bytes())
}

/// `AttentionTracker::target` (`AttentionTracker.cpp:270-277`).
pub fn target(token: &str) -> Option<Map<String, Value>> {
    if token.is_empty() || token.len() > 8192 {
        return None;
    }
    let Value::Object(object) = serde_json::from_slice(&from_base64url(token)?).ok()? else {
        return None;
    };
    let valid = object.get("host").is_some_and(Value::is_string)
        && !text(&object, "name").is_empty()
        && !text(&object, "notice").is_empty();
    valid.then_some(object)
}

/// `AttentionTracker::matches` (`AttentionTracker.cpp:279-287`).
pub fn matches(target: &Map<String, Value>, row: &ViewRow) -> bool {
    if target.is_empty() || row.state == "archived" {
        return false;
    }
    if !text(target, "reply").is_empty() || !text(target, "question").is_empty() {
        return text(target, "conversation") == row.conversation_id.as_deref().unwrap_or("")
            && text(target, "agent") == row.notify.cmd;
    }
    match text(target, "run") {
        "" => text(target, "name") == row.name && integer(target, "created") == row.notify.created,
        run => Some(run) == row.run_id.as_deref(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Open { computer: ComputerId, key: String },
    Gone(String),
    Waiting,
}

/// Where a clicked notification leads (`TrayAgent::resolveAttentionClick`):
/// its session, or gone once a current listing lacks it or [`CLICK_WAIT`]
/// passed (`expired`). Qt asks for a new listing at the click because its
/// rows can be a poll old; a computer whose stream is current already is
/// that listing.
pub fn resolve(target: &Map<String, Value>, desk: &Desk, expired: bool) -> Resolution {
    let computer = ComputerId(text(target, "host").to_owned());
    let state = desk.computers().get(&computer);
    if let Some(found) =
        state.and_then(|state| state.rows.iter().find(|(_, row)| matches(target, row)))
    {
        return Resolution::Open {
            computer,
            key: found.0.clone(),
        };
    }
    if expired || state.is_some_and(Computer::fresh) {
        Resolution::Gone(text(target, "name").to_owned())
    } else {
        Resolution::Waiting
    }
}

pub fn gone_text(name: &str) -> String {
    format!("This session is no longer available: {name}")
}

/// The tracker's saved state: the client's own file; on its first start,
/// Qt's `attention/observed` value (Decision 11).
pub fn seed(own: Option<&[u8]>, qt: Option<&[u8]>) -> Map<String, Value> {
    let parse = |bytes: &[u8]| match serde_json::from_slice(bytes) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    };
    match own {
        Some(bytes) => parse(bytes).unwrap_or_default(),
        None => qt.and_then(parse).unwrap_or_default(),
    }
}

/// `SessionNotifications::valid` (`SessionNotifications.h:44-77`), the check
/// the view rows already apply ([`crate::view::preferences_valid`]).
pub fn valid_preferences(value: &Value, conversation: &str) -> bool {
    value
        .as_object()
        .is_some_and(|object| crate::view::preferences_valid(object, conversation))
}

fn identity(row: &ViewRow) -> String {
    match nonempty(&row.run_id) {
        Some(run) => format!("run:{run}"),
        None => format!("name:{}:{}", row.name, row.notify.created),
    }
}

fn muted(row: &ViewRow) -> bool {
    row.notify
        .preferences
        .as_ref()
        .is_some_and(|preferences| preferences.muted)
}

fn baseline(row: &ViewRow) -> Option<&Map<String, Value>> {
    row.notify
        .preferences
        .as_ref()
        .and_then(|preferences| preferences.suppressed.as_object())
}

/// `SessionNotifications::matches` (`SessionNotifications.h:81-84`).
fn matches_saved(
    saved: &Map<String, Value>,
    phase: &str,
    request: &str,
    started: Option<f64>,
) -> bool {
    text(saved, "phase") == phase
        && text(saved, "request_id") == request
        && (!request.is_empty()
            || started
                .and_then(positive)
                .is_some_and(|at| saved.get("started_at").and_then(Value::as_f64) == Some(at)))
}

fn suppressed_main(row: &ViewRow, baseline: &Map<String, Value>) -> bool {
    baseline
        .get("main_attention")
        .and_then(Value::as_object)
        .is_some_and(|main| {
            !main.is_empty()
                && text(main, "run_id") == row.run_id.as_deref().unwrap_or("")
                && matches_saved(
                    main,
                    row.phase.as_deref().unwrap_or(""),
                    &row.notify.attention_id,
                    Some(row.notify.attention_started),
                )
        })
}

fn suppressed_child(baseline: &Map<String, Value>, child: &crate::view::ChildFacts) -> bool {
    baseline
        .get("child_attention")
        .and_then(Value::as_object)
        .and_then(|children| children.get(&child.id))
        .and_then(Value::as_object)
        .is_some_and(|saved| {
            !saved.is_empty() && matches_saved(saved, &child.phase, &child.request, child.started)
        })
}

/// `SessionPresentation::optionalQuestionCount`.
fn optional_count(row: &ViewRow) -> i64 {
    if row.state != "running" || row.process_state == "exited" || row.notify.optional_count < 0 {
        0
    } else {
        row.notify.optional_count
    }
}

/// One computer as the tracker reads it.
pub struct Host<'a> {
    /// "" for this computer, else the peer's alias: the state's key and the token's `host`.
    pub alias: &'a str,
    /// The name notices show: the alias, or this computer's host name.
    pub label: &'a str,
    /// A current listing (`box.ok`); otherwise only muted banners are withdrawn.
    pub ok: bool,
    pub rows: &'a BTreeMap<String, ViewRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub token: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub raised: Vec<Notice>,
    pub cleared: Vec<String>,
}

struct Edge {
    key: String,
    phase: String,
    request: String,
    reason: String,
    reply: bool,
    silent: bool,
    started: Option<f64>,
}

struct Pass<'a, 'n> {
    host: &'a Host<'a>,
    previous: &'a Map<String, Value>,
    current: Map<String, Value>,
    raised: Vec<Notice>,
    now: i64,
    focused: bool,
    notice_id: &'n mut dyn FnMut() -> String,
}

impl Pass<'_, '_> {
    fn body(&self, row: &ViewRow, reason: &str) -> String {
        format!(
            "{} / {}\n{reason}",
            concise(&row.name, 120),
            concise(self.host.label, 40)
        )
    }

    fn target(&mut self, row: &ViewRow) -> Map<String, Value> {
        let mut target = Map::new();
        target.insert("host".into(), Value::from(self.host.alias));
        target.insert("name".into(), Value::from(row.name.as_str()));
        target.insert(
            "run".into(),
            Value::from(row.run_id.as_deref().unwrap_or("")),
        );
        target.insert("created".into(), Value::from(row.notify.created));
        target.insert("notice".into(), Value::from((self.notice_id)()));
        target
    }

    /// `notice` (`AttentionTracker.cpp:89-126`).
    fn notice(&mut self, row: &ViewRow, edge: Edge) {
        let empty = Map::new();
        // A key never seen reads as an empty entry, as `QJsonObject::value`
        // gives one.
        let old = self
            .previous
            .get(&edge.key)
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let mut entry = Map::new();
        entry.insert("phase".into(), Value::from(edge.phase.as_str()));
        entry.insert("request".into(), Value::from(edge.request.as_str()));
        if let Some(started) = edge.started {
            entry.insert("started".into(), qt_value(started));
        }
        // An identity-less main request is distinguished by its start time.
        // Entries from before this field was kept match their request once.
        let same = text(old, "phase") == edge.phase
            && text(old, "request") == edge.request
            && (edge.started.is_none()
                || !old.contains_key("started")
                || old.get("started").and_then(Value::as_f64).unwrap_or(0.0)
                    == edge.started.unwrap_or(0.0));
        if edge.silent || muted(row) {
            // Withdraw the banner but remember the request without a token.
            self.current.insert(edge.key, Value::Object(entry));
            return;
        }
        let retry = integer(old, "retry_at");
        if same && (retry == 0 || retry > self.now) {
            let mut kept = old.clone();
            if let Some(started) = edge.started {
                kept.insert("started".into(), qt_value(started));
            }
            self.current.insert(edge.key, Value::Object(kept));
            return;
        }
        let mut target = self.target(row);
        if edge.reply {
            target.insert(
                "reply".into(),
                Value::from(row.reply_id.as_deref().unwrap_or("")),
            );
            target.insert(
                "conversation".into(),
                Value::from(row.conversation_id.as_deref().unwrap_or("")),
            );
            target.insert("agent".into(), Value::from(row.notify.cmd.as_str()));
        }
        let token = encode_token(&target);
        entry.insert("token".into(), Value::from(token.as_str()));
        self.current.insert(edge.key, Value::Object(entry));
        // Consume foreground edges without changing unread or action state.
        if !self.focused {
            let title = if edge.reply {
                NEW_REPLY
            } else {
                NEEDS_ATTENTION
            };
            let body = self.body(row, &edge.reason);
            self.raised.push(Notice {
                token,
                title: title.to_owned(),
                body,
            });
        }
    }

    /// One session's request, reply and children (`AttentionTracker.cpp:127-171`).
    fn session(&mut self, row: &ViewRow) {
        let facts = &row.notify;
        let baseline = baseline(row);
        if facts.needs_action {
            let reason = match row.phase.as_deref() {
                Some("approval") => "Approval needed".to_owned(),
                Some("input") => "Input needed".to_owned(),
                _ if !facts.recovery_reason.is_empty() => concise(&facts.recovery_reason, 240),
                _ if !facts.activity_detail.is_empty() => concise(&facts.activity_detail, 240),
                _ => "Agent needs attention".to_owned(),
            };
            let request = match baseline {
                Some(_) => request_json(&facts.attention_id, Some(facts.attention_started)),
                None => facts.attention_id.clone(),
            };
            self.notice(
                row,
                Edge {
                    key: identity(row),
                    phase: row.phase.clone().unwrap_or_default(),
                    request,
                    reason,
                    reply: false,
                    silent: facts.acknowledged || baseline.is_some_and(|b| suppressed_main(row, b)),
                    started: facts
                        .attention_id
                        .is_empty()
                        .then_some(facts.attention_started),
                },
            );
        }
        // Replies belong to conversations so a rename or resume does not
        // repeat a banner.
        if let (true, Some(reply), Some(conversation)) = (
            row.state != "archived" && facts.unread_reply,
            nonempty(&row.reply_id),
            nonempty(&row.conversation_id),
        ) {
            let key = format!("reply:{}:{conversation}", facts.cmd);
            // A reply posts once the session is confirmed idle.
            let idle = row.activity == "idle"
                && row.phase.as_deref() == Some("idle")
                && facts.background <= 0
                && facts.subagents_active == 0
                && !facts.coordination_wait
                && !facts.needs_action;
            let suppressed = baseline
                .and_then(|b| b.get("reply_id"))
                .and_then(Value::as_str)
                == Some(reply);
            let edge = |reason: &str, silent: bool| Edge {
                key: key.clone(),
                phase: "reply".to_owned(),
                request: reply.to_owned(),
                reason: reason.to_owned(),
                reply: true,
                silent,
                started: None,
            };
            let previous = self.previous;
            // Routine ticks of a recurring scheduled job are consumed.
            if muted(row)
                || suppressed
                || (idle && facts.reply_scheduled && facts.recurring_schedules > 0)
            {
                self.notice(row, edge("", true));
            } else if idle && (!facts.reply_scheduled || facts.recurring_schedules == 0) {
                self.notice(row, edge("Agent has a new reply", false));
            } else if let Some(kept) = previous
                .get(&key)
                .filter(|old| old.get("request").and_then(Value::as_str) == Some(reply))
            {
                self.current.insert(key.clone(), kept.clone());
            }
        }
        if row.state != "running"
            || row.process_state == "exited"
            || row.activity == "unknown"
            || (facts.conversation_state == "ended" && row.activity != "idle")
            || facts.subagent_source == "hook_profiles"
        {
            return;
        }
        for child in &facts.children {
            let request = match baseline {
                Some(_) => request_json(&child.request, child.started),
                None => child.request.clone(),
            };
            let kind = if child.phase == "approval" {
                "Approval needed"
            } else {
                "Input needed"
            };
            self.notice(
                row,
                Edge {
                    key: format!("child:{}:{}", identity(row), child.id),
                    phase: child.phase.clone(),
                    request,
                    reason: format!("Subagent {}: {kind}", concise(&child.label, 100)),
                    reply: false,
                    silent: facts.acknowledged
                        || baseline.is_some_and(|b| suppressed_child(b, child)),
                    started: None,
                },
            );
        }
    }

    /// Optional questions per conversation (`AttentionTracker.cpp:172-242`).
    fn questions(&mut self, enabled: bool) -> Vec<Notice> {
        struct Optional<'r> {
            row: &'r ViewRow,
            ids: Vec<String>,
            unknown: bool,
        }
        let live = |row: &ViewRow| row.state == "running" && row.process_state != "exited";
        let rows = self.host.rows;
        let mut optional: BTreeMap<String, Optional> = BTreeMap::new();
        for row in rows.values() {
            let Some(conversation) = nonempty(&row.conversation_id) else {
                continue;
            };
            if !row.notify.optional_reported {
                continue;
            }
            let entry = optional
                .entry(format!("question:{}:{conversation}", row.notify.cmd))
                .or_insert(Optional {
                    row,
                    ids: Vec::new(),
                    unknown: false,
                });
            if live(row) && !live(entry.row) {
                entry.row = row;
            }
            if live(row) && row.notify.optional_count < 0 {
                entry.unknown = true;
            }
            if optional_count(row) > 0 {
                for id in &row.notify.optional_ids {
                    if !entry.ids.contains(id) {
                        entry.ids.push(id.clone());
                    }
                }
            }
        }
        let previous = self.previous;
        let mut questions = Vec::new();
        for (key, item) in &optional {
            let row = item.row;
            let mut old = previous
                .get(key)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let seen_baseline: Vec<Value> = baseline(row)
                .and_then(|b| b.get("optional_question_ids"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !seen_baseline.is_empty() {
                if let Some(posted) = target(text(&old, "token")) {
                    if text(&posted, "host") == self.host.alias
                        && text(&posted, "agent") == row.notify.cmd
                        && text(&posted, "conversation")
                            == row.conversation_id.as_deref().unwrap_or("")
                        && seen_baseline.contains(posted.get("question").unwrap_or(&Value::Null))
                    {
                        old.remove("token");
                        old.remove("retry_at");
                    }
                }
            }
            if item.unknown {
                // An unreadable index is not an answer: keep the banner and IDs.
                if !old.is_empty() {
                    if muted(row) {
                        old.remove("token");
                        old.remove("retry_at");
                    }
                    self.current.insert(key.clone(), Value::Object(old));
                }
                continue;
            }
            let mut known = BTreeSet::new();
            let mut seen = Vec::new();
            for id in old
                .get("seen")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                known.insert(id.to_owned());
                if !item.ids.iter().any(|current| current == id) {
                    seen.push(id.to_owned());
                }
            }
            // Baseline membership suppresses old questions without evicting a
            // newer observed question from the bounded seen IDs.
            known.extend(
                seen_baseline
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            );
            let fresh: Vec<&String> = item.ids.iter().filter(|id| !known.contains(*id)).collect();
            let drop = (seen.len() + item.ids.len()).saturating_sub(SEEN_QUESTIONS);
            let mut seen: Vec<String> = seen.into_iter().skip(drop).collect();
            seen.extend(item.ids.iter().cloned());
            let mut entry = Map::new();
            entry.insert("seen".into(), json!(seen));
            // Marking the session read acknowledges its questions; turning
            // banners off or muting withdraws them.
            if !item.ids.is_empty() && enabled && !row.notify.acknowledged && !muted(row) {
                let token = text(&old, "token").to_owned();
                let retry = integer(&old, "retry_at");
                // A request that needs action announces itself; with focus,
                // new questions are only recorded.
                if (!fresh.is_empty() || (!token.is_empty() && retry != 0 && retry <= self.now))
                    && !self.focused
                    && !row.notify.needs_action
                {
                    let question = fresh
                        .last()
                        .map(|id| (*id).clone())
                        .or_else(|| item.ids.last().cloned())
                        .unwrap_or_default();
                    let mut target = self.target(row);
                    target.insert("question".into(), Value::from(question));
                    target.insert(
                        "conversation".into(),
                        Value::from(row.conversation_id.as_deref().unwrap_or("")),
                    );
                    target.insert("agent".into(), Value::from(row.notify.cmd.as_str()));
                    let token = encode_token(&target);
                    entry.insert("token".into(), Value::from(token.as_str()));
                    let count = row.notify.optional_count.max(item.ids.len() as i64);
                    let reason = if count == 1 {
                        "Optional question waiting".to_owned()
                    } else {
                        format!("{count} optional questions waiting")
                    };
                    questions.push(Notice {
                        token,
                        title: OPTIONAL_QUESTION.to_owned(),
                        body: self.body(row, &reason),
                    });
                } else if !token.is_empty() {
                    entry.insert("token".into(), Value::from(token));
                    if retry != 0 {
                        entry.insert("retry_at".into(), Value::from(retry));
                    }
                }
            }
            self.current.insert(key.clone(), Value::Object(entry));
        }
        questions
    }
}

/// Edges per computer, kept across restarts (`AttentionTracker`).
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    hosts: Map<String, Value>,
    optional_questions: bool,
    /// The state changed since [`Tracker::take_changed`] last asked.
    changed: bool,
}

impl Tracker {
    pub fn new(saved: Map<String, Value>) -> Self {
        Self {
            hosts: saved,
            optional_questions: false,
            changed: false,
        }
    }

    /// Banners for new optional questions (off by default). Without them
    /// their IDs are still recorded, so turning them on announces only later
    /// ones. The caller reads the setting (`notifications/optionalQuestions`).
    pub fn set_optional_questions(&mut self, on: bool) {
        self.optional_questions = on;
    }

    pub fn state(&self) -> &Map<String, Value> {
        &self.hosts
    }

    /// Whether the state changed since the last call: Qt stores it after an
    /// observation that changed it (`TrayAgent.cpp:553-557`) and after a
    /// failed delivery (`TrayAgent.cpp:198-199`). Answers without copying it.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// `AttentionTracker::observe` (`AttentionTracker.cpp:51-253`).
    pub fn observe(
        &mut self,
        host: &Host,
        now: i64,
        focused: bool,
        notice_id: &mut dyn FnMut() -> String,
    ) -> Changes {
        // Taken, not copied: every path below puts the host's entry back.
        let existed = self.hosts.get(host.alias).is_some_and(Value::is_object);
        let previous = match self.hosts.get_mut(host.alias) {
            Some(Value::Object(entries)) => std::mem::take(entries),
            _ => Map::new(),
        };
        if !host.ok {
            return self.observe_offline(host, previous, existed);
        }
        let mut pass = Pass {
            host,
            previous: &previous,
            current: Map::new(),
            raised: Vec::new(),
            now,
            focused,
            notice_id,
        };
        for row in host.rows.values() {
            pass.session(row);
        }
        // Posted first, as Qt does (`AttentionTracker.cpp:243-245`).
        let mut raised = pass.questions(self.optional_questions);
        raised.append(&mut pass.raised);
        let current = pass.current;
        let mut changes = Changes {
            raised,
            cleared: Vec::new(),
        };
        for (key, value) in &previous {
            let token = value.get("token").and_then(Value::as_str).unwrap_or("");
            if !token.is_empty()
                && current
                    .get(key)
                    .and_then(|entry| entry.get("token"))
                    .and_then(Value::as_str)
                    != Some(token)
            {
                changes.cleared.push(token.to_owned());
            }
        }
        // Qt assigns the host's entries every time; a new host's empty ones too.
        self.changed |= !existed || current != previous;
        self.hosts
            .insert(host.alias.to_owned(), Value::Object(current));
        changes
    }

    /// A failed or stale listing only withdraws muted banners and keeps every
    /// identity (`AttentionTracker.cpp:55-81`).
    fn observe_offline(
        &mut self,
        host: &Host,
        mut retained: Map<String, Value>,
        existed: bool,
    ) -> Changes {
        let mut changes = Changes::default();
        let mut muted_runs = BTreeSet::new();
        let mut muted_conversations = BTreeSet::new();
        for row in host
            .rows
            .values()
            .filter(|row| muted(row) && row.state != "archived")
        {
            muted_runs.insert(identity(row));
            if let Some(conversation) = nonempty(&row.conversation_id) {
                muted_conversations.insert(format!("{}:{conversation}", row.notify.cmd));
            }
        }
        for entry in retained.values_mut().filter_map(Value::as_object_mut) {
            let token = text(entry, "token").to_owned();
            let Some(destination) = target(&token) else {
                continue;
            };
            if text(&destination, "host") != host.alias {
                continue;
            }
            let conversation = !text(&destination, "reply").is_empty()
                || !text(&destination, "question").is_empty();
            let is_muted = if conversation {
                muted_conversations.contains(&format!(
                    "{}:{}",
                    text(&destination, "agent"),
                    text(&destination, "conversation")
                ))
            } else {
                let key = match text(&destination, "run") {
                    "" => format!(
                        "name:{}:{}",
                        text(&destination, "name"),
                        integer(&destination, "created")
                    ),
                    run => format!("run:{run}"),
                };
                muted_runs.contains(&key)
            };
            if is_muted {
                changes.cleared.push(token);
                entry.remove("token");
                entry.remove("retry_at");
            }
        }
        // A host never observed stays absent, as Qt writes only a change.
        self.changed |= !changes.cleared.is_empty();
        if existed || !changes.cleared.is_empty() {
            self.hosts
                .insert(host.alias.to_owned(), Value::Object(retained));
        }
        changes
    }

    /// `AttentionTracker::deliveryFailed` (`AttentionTracker.cpp:255-268`).
    pub fn delivery_failed(&mut self, token: &str, now: i64) {
        for entries in self.hosts.values_mut().filter_map(Value::as_object_mut) {
            for entry in entries.values_mut().filter_map(Value::as_object_mut) {
                if entry.get("token").and_then(Value::as_str) == Some(token) {
                    entry.insert("retry_at".into(), Value::from(now + RETRY_SECONDS));
                    self.changed = true;
                    return;
                }
            }
        }
    }
}

/// The notification request's identifier (`MacAttentionNotifier.mm:79`, `:102`).
pub fn request_id(token: &str) -> String {
    format!("zerus.attention.{token}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryStep {
    CheckAuthorization,
    Add {
        token: String,
        title: String,
        body: String,
    },
    Remove {
        token: String,
    },
    Fail {
        token: String,
        detail: String,
    },
}

/// The macOS notifier's state (`MacAttentionNotifier.mm:51-114`): one
/// authorization check at a time; tokens withdrawn meanwhile are not posted.
#[derive(Debug, Default)]
pub struct Delivery {
    desired: BTreeSet<String>,
    pending: BTreeMap<String, (String, String)>,
    checking: bool,
}

impl Delivery {
    /// Posts once per token; asks for the authorization unless a check runs.
    pub fn post(&mut self, token: &str, title: &str, body: &str) -> Vec<DeliveryStep> {
        if !self.desired.insert(token.to_owned()) {
            return Vec::new();
        }
        self.pending
            .insert(token.to_owned(), (title.to_owned(), body.to_owned()));
        if self.checking {
            return Vec::new();
        }
        self.checking = true;
        vec![DeliveryStep::CheckAuthorization]
    }

    pub fn withdraw(&mut self, token: &str) -> Vec<DeliveryStep> {
        self.desired.remove(token);
        self.pending.remove(token);
        vec![DeliveryStep::Remove {
            token: token.to_owned(),
        }]
    }

    /// The check finished: adds what is still wanted, or fails every pending
    /// token without permission.
    pub fn authorized(&mut self, granted: bool, error: Option<String>) -> Vec<DeliveryStep> {
        self.checking = false;
        let pending = std::mem::take(&mut self.pending);
        if !granted {
            let detail = error
                .filter(|error| !error.is_empty())
                .unwrap_or_else(|| DENIED.to_owned());
            return pending
                .into_keys()
                .map(|token| {
                    self.desired.remove(&token);
                    DeliveryStep::Fail {
                        token,
                        detail: detail.clone(),
                    }
                })
                .collect();
        }
        pending
            .into_iter()
            .filter(|(token, _)| self.desired.contains(token))
            .map(|(token, (title, body))| DeliveryStep::Add { token, title, body })
            .collect()
    }

    /// An add finished: a token withdrawn meanwhile goes, an error fails it.
    pub fn added(&mut self, token: &str, error: Option<String>) -> Vec<DeliveryStep> {
        if !self.desired.contains(token) {
            return self.withdraw(token);
        }
        match error.filter(|error| !error.is_empty()) {
            Some(detail) => {
                self.desired.remove(token);
                vec![DeliveryStep::Fail {
                    token: token.to_owned(),
                    detail,
                }]
            }
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{ChildFacts, Preferences};
    use crate::{Change, ComputerStatus};

    fn session(name: &str) -> ViewRow {
        let mut row = ViewRow {
            name: name.to_owned(),
            state: "running".to_owned(),
            activity: "idle".to_owned(),
            phase: Some("idle".to_owned()),
            run_id: Some(format!("run-{name}")),
            conversation_id: Some(format!("conv-{name}")),
            ..ViewRow::default()
        };
        row.notify.cmd = "claude".to_owned();
        row.notify.created = 100;
        row.notify.background = -1;
        row.notify.recurring_schedules = -1;
        row.notify.optional_count = -1;
        row
    }

    fn asking(name: &str, phase: &str, request: &str) -> ViewRow {
        let mut row = session(name);
        row.phase = Some(phase.to_owned());
        row.activity = "waiting".to_owned();
        row.notify.needs_action = true;
        row.notify.attention_id = request.to_owned();
        row
    }

    fn replied(name: &str, reply: &str) -> ViewRow {
        let mut row = session(name);
        row.reply_id = Some(reply.to_owned());
        row.notify.unread_reply = true;
        row
    }

    fn with_preferences(mut row: ViewRow, muted: bool) -> ViewRow {
        row.notify.preferences = Some(Preferences {
            muted,
            suppressed: json!({"reply_id": null, "attention_signature": null, "optional_question_ids": [],
                               "main_attention": null, "child_attention": {}}),
        });
        row
    }

    struct Fixture {
        tracker: Tracker,
        notices: u32,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                tracker: Tracker::new(Map::new()),
                notices: 0,
            }
        }

        fn observe_at(&mut self, list: Vec<ViewRow>, now: i64, focused: bool, ok: bool) -> Changes {
            let rows: BTreeMap<String, ViewRow> = list
                .into_iter()
                .map(|row| (row.name.clone(), row))
                .collect();
            let mut count = self.notices;
            let changes = self.tracker.observe(
                &Host {
                    alias: "",
                    label: "arch",
                    ok,
                    rows: &rows,
                },
                now,
                focused,
                &mut || {
                    count += 1;
                    format!("notice-{count}")
                },
            );
            self.notices = count;
            changes
        }

        fn observe(&mut self, list: Vec<ViewRow>) -> Changes {
            self.observe_at(list, 1000, false, true)
        }
    }

    #[test]
    fn an_approval_notifies_once_and_is_withdrawn_when_answered() {
        // edgesOfflineRestartAndIdentity
        let mut fixture = Fixture::new();
        let changes = fixture.observe(vec![asking("claude/p/a", "approval", "req-1")]);
        assert_eq!(changes.raised.len(), 1);
        assert_eq!(changes.raised[0].title, NEEDS_ATTENTION);
        assert_eq!(changes.raised[0].body, "claude/p/a / arch\nApproval needed");
        let token = changes.raised[0].token.clone();
        assert_eq!(
            fixture.observe(vec![asking("claude/p/a", "approval", "req-1")]),
            Changes::default()
        );
        assert_eq!(
            fixture.observe(vec![session("claude/p/a")]),
            Changes {
                raised: vec![],
                cleared: vec![token]
            }
        );
    }

    #[test]
    fn the_tracker_says_when_its_state_changed_as_qt_compares_it() {
        // `TrayAgent::observeAttention` stores the state only when it changed.
        let mut fixture = Fixture::new();
        assert!(!fixture.tracker.take_changed(), "nothing observed yet");
        fixture.observe(vec![session("claude/p/a")]);
        assert!(
            fixture.tracker.take_changed(),
            "a host seen for the first time gets its entry, even an empty one"
        );
        assert!(!fixture.tracker.take_changed(), "asked once");
        fixture.observe(vec![session("claude/p/a")]);
        assert!(!fixture.tracker.take_changed(), "the same listing");
        let raised = fixture.observe(vec![asking("claude/p/a", "approval", "req-1")]);
        assert!(fixture.tracker.take_changed(), "a new edge");
        fixture.observe(vec![asking("claude/p/a", "approval", "req-1")]);
        assert!(!fixture.tracker.take_changed(), "the same edge");
        fixture.tracker.delivery_failed("unknown-token", 1000);
        assert!(!fixture.tracker.take_changed(), "no entry has that token");
        fixture
            .tracker
            .delivery_failed(&raised.raised[0].token, 1000);
        assert!(fixture.tracker.take_changed(), "a retry is due");
        // An offline listing changes the state only when it withdraws a muted banner.
        fixture.observe_at(
            vec![asking("claude/p/a", "approval", "req-1")],
            1001,
            false,
            false,
        );
        assert!(
            !fixture.tracker.take_changed(),
            "offline keeps every identity"
        );
        let muted = with_preferences(asking("claude/p/a", "approval", "req-1"), true);
        assert_eq!(
            fixture
                .observe_at(vec![muted], 1002, false, false)
                .cleared
                .len(),
            1
        );
        assert!(
            fixture.tracker.take_changed(),
            "the muted banner's token is gone"
        );
        let mut offline = Fixture::new();
        offline.observe_at(vec![session("claude/p/a")], 1000, false, false);
        assert!(
            !offline.tracker.take_changed(),
            "a host never observed stays absent offline"
        );
    }

    #[test]
    fn a_focused_window_consumes_the_edge_without_a_banner() {
        // foregroundRequestsStayActionableWithoutReplay
        let mut fixture = Fixture::new();
        assert!(fixture
            .observe_at(
                vec![asking("claude/p/a", "input", "req-1")],
                1000,
                true,
                true
            )
            .raised
            .is_empty());
        assert!(fixture
            .observe(vec![asking("claude/p/a", "input", "req-1")])
            .raised
            .is_empty());
        assert_eq!(
            fixture
                .observe(vec![asking("claude/p/a", "input", "req-2")])
                .raised
                .len(),
            1
        );
    }

    #[test]
    fn a_reply_waits_for_idle_and_notifies_once() {
        // replyWaitsForLiveBackgroundShellToFinish, deliveredRepliesDoNotRepeatAfterWorking
        let mut fixture = Fixture::new();
        let mut busy = replied("claude/p/a", "rep-1");
        busy.activity = "busy".to_owned();
        busy.phase = Some("working".to_owned());
        assert_eq!(fixture.observe(vec![busy.clone()]), Changes::default());
        let mut background = replied("claude/p/a", "rep-1");
        background.notify.background = 1;
        assert_eq!(fixture.observe(vec![background]), Changes::default());
        let idle = fixture.observe(vec![replied("claude/p/a", "rep-1")]);
        assert_eq!(idle.raised.len(), 1);
        assert_eq!(idle.raised[0].title, NEW_REPLY);
        assert_eq!(
            idle.raised[0].body,
            "claude/p/a / arch\nAgent has a new reply"
        );
        assert_eq!(
            fixture.observe(vec![busy]),
            Changes::default(),
            "working again keeps the delivered reply"
        );
        assert_eq!(
            fixture.observe(vec![replied("claude/p/a", "rep-1")]),
            Changes::default()
        );
    }

    #[test]
    fn routine_ticks_of_a_recurring_schedule_are_consumed() {
        let mut fixture = Fixture::new();
        let mut tick = replied("claude/p/a", "rep-1");
        tick.notify.reply_scheduled = true;
        tick.notify.recurring_schedules = 2;
        assert!(fixture.observe(vec![tick]).raised.is_empty());
        let mut last = replied("claude/p/a", "rep-2");
        last.notify.reply_scheduled = true;
        last.notify.recurring_schedules = 0;
        assert_eq!(fixture.observe(vec![last]).raised.len(), 1);
    }

    #[test]
    fn muted_sessions_withdraw_their_banners_and_unmuting_never_replays() {
        // muteConsumesAllNotificationEdgesWithoutAcknowledging
        let mut fixture = Fixture::new();
        let token = fixture
            .observe(vec![with_preferences(
                asking("claude/p/a", "approval", "r"),
                false,
            )])
            .raised[0]
            .token
            .clone();
        let muted = fixture.observe(vec![with_preferences(
            asking("claude/p/a", "approval", "r"),
            true,
        )]);
        assert_eq!(
            muted,
            Changes {
                raised: vec![],
                cleared: vec![token]
            }
        );
        assert!(fixture
            .observe(vec![with_preferences(
                asking("claude/p/a", "approval", "r"),
                false
            )])
            .raised
            .is_empty());
    }

    #[test]
    fn an_offline_host_withdraws_only_muted_banners() {
        // muteWithdrawsOnlyMatchingOfflineBannersWithoutReplay
        let mut fixture = Fixture::new();
        let raised = fixture
            .observe(vec![
                asking("claude/p/a", "approval", "r"),
                asking("claude/p/b", "approval", "r"),
            ])
            .raised;
        let token = raised
            .iter()
            .find(|notice| notice.body.starts_with("claude/p/a"))
            .unwrap()
            .token
            .clone();
        let offline = fixture.observe_at(
            vec![
                with_preferences(asking("claude/p/a", "approval", "r"), true),
                asking("claude/p/b", "approval", "r"),
            ],
            1000,
            false,
            false,
        );
        assert_eq!(
            offline,
            Changes {
                raised: vec![],
                cleared: vec![token]
            }
        );
        assert!(fixture
            .observe(vec![asking("claude/p/b", "approval", "r")])
            .raised
            .is_empty());
    }

    #[test]
    fn an_acknowledged_request_is_withdrawn() {
        // acknowledgedAttentionWithdrawsItsNotification
        let mut fixture = Fixture::new();
        let token = fixture
            .observe(vec![asking("claude/p/a", "approval", "r")])
            .raised[0]
            .token
            .clone();
        let mut read = asking("claude/p/a", "approval", "r");
        read.notify.acknowledged = true;
        assert_eq!(
            fixture.observe(vec![read]),
            Changes {
                raised: vec![],
                cleared: vec![token]
            }
        );
    }

    #[test]
    fn child_requests_notify_with_their_label() {
        // onlyExplicitChildRequestsNotify
        let mut fixture = Fixture::new();
        let mut parent = session("claude/p/a");
        parent.activity = "busy".to_owned();
        parent.notify.children = vec![ChildFacts {
            id: "w1".into(),
            label: "Reviewer".into(),
            phase: "approval".into(),
            request: "q-1".into(),
            started: Some(5.0),
        }];
        let changes = fixture.observe(vec![parent.clone()]);
        assert_eq!(changes.raised.len(), 1);
        assert_eq!(
            changes.raised[0].body,
            "claude/p/a / arch\nSubagent Reviewer: Approval needed"
        );
        parent.notify.subagent_source = "hook_profiles".into();
        assert_eq!(fixture.observe(vec![parent]).cleared.len(), 1);
    }

    #[test]
    fn optional_questions_are_recorded_and_notify_only_when_enabled() {
        // optionalQuestionsNotifyOncePerQuestionWhenEnabled
        let mut fixture = Fixture::new();
        let mut asked = session("codex/p/a");
        asked.notify.cmd = "codex".into();
        asked.notify.optional_reported = true;
        asked.notify.optional_count = 1;
        asked.notify.optional_ids = vec!["q1".into()];
        assert!(
            fixture.observe(vec![asked.clone()]).raised.is_empty(),
            "off by default; q1 is recorded"
        );
        fixture.tracker.set_optional_questions(true);
        assert!(
            fixture.observe(vec![asked.clone()]).raised.is_empty(),
            "turning banners on announces only later ones"
        );
        asked.notify.optional_count = 2;
        asked.notify.optional_ids = vec!["q1".into(), "q2".into()];
        let changes = fixture.observe(vec![asked]);
        assert_eq!(changes.raised.len(), 1);
        assert_eq!(changes.raised[0].title, OPTIONAL_QUESTION);
        assert_eq!(
            changes.raised[0].body,
            "codex/p/a / arch\n2 optional questions waiting"
        );
        assert_eq!(target(&changes.raised[0].token).unwrap()["question"], "q2");
    }

    #[test]
    fn a_failed_delivery_retries_after_a_minute() {
        // failedDeliveryRetriesAfterBackoff
        let mut fixture = Fixture::new();
        let token = fixture
            .observe(vec![asking("claude/p/a", "approval", "r")])
            .raised[0]
            .token
            .clone();
        fixture.tracker.delivery_failed(&token, 1000);
        assert!(fixture
            .observe_at(
                vec![asking("claude/p/a", "approval", "r")],
                1059,
                false,
                true
            )
            .raised
            .is_empty());
        let retried = fixture.observe_at(
            vec![asking("claude/p/a", "approval", "r")],
            1060,
            false,
            true,
        );
        assert_eq!(retried.raised.len(), 1);
        assert_ne!(retried.raised[0].token, token);
        assert_eq!(retried.cleared, vec![token]);
    }

    #[test]
    fn targets_match_by_conversation_run_or_name_and_creation() {
        let mut fixture = Fixture::new();
        let reply =
            target(&fixture.observe(vec![replied("claude/p/a", "rep-1")]).raised[0].token).unwrap();
        let mut renamed = replied("claude/p/renamed", "rep-1");
        renamed.conversation_id = Some("conv-claude/p/a".into());
        assert!(
            matches(&reply, &renamed),
            "a reply follows its conversation across a rename"
        );
        let mut other_agent = renamed.clone();
        other_agent.notify.cmd = "codex".into();
        assert!(!matches(&reply, &other_agent));
        let mut archived = renamed.clone();
        archived.state = "archived".into();
        assert!(!matches(&reply, &archived), "an archived row never opens");
        let request = target(
            &fixture
                .observe(vec![asking("claude/p/b", "approval", "r")])
                .raised[0]
                .token,
        )
        .unwrap();
        let mut resumed = asking("claude/p/b", "approval", "r");
        resumed.run_id = Some("another-run".into());
        assert!(!matches(&request, &resumed), "a request belongs to its run");
        let mut runless = asking("claude/p/c", "approval", "r");
        runless.run_id = None;
        let by_name = target(&fixture.observe(vec![runless.clone()]).raised[0].token).unwrap();
        assert!(matches(&by_name, &runless));
        runless.notify.created = 101;
        assert!(
            !matches(&by_name, &runless),
            "a run-less session is its name and creation time"
        );
        assert_eq!(target(&"x".repeat(8193)), None);
        assert_eq!(target("not base64 !"), None);
    }

    #[test]
    fn a_click_waits_then_reports_a_gone_session() {
        let token = Fixture::new()
            .observe(vec![asking("claude/p/a", "approval", "r")])
            .raised[0]
            .token
            .clone();
        let wanted = target(&token).unwrap();
        let mut desk = Desk::default();
        desk.observe(&Change::Computer {
            computer: ComputerId::local(),
            status: ComputerStatus::Probing,
        });
        assert_eq!(
            resolve(&wanted, &desk, false),
            Resolution::Waiting,
            "the computer has not answered yet"
        );
        assert_eq!(
            resolve(&wanted, &desk, true),
            Resolution::Gone("claude/p/a".into()),
            "16 s later it is gone"
        );
        desk.observe(&Change::Replace {
            computer: ComputerId::local(),
            rows: vec![("k-a".into(), asking("claude/p/a", "approval", "r"))],
        });
        desk.observe(&Change::Computer {
            computer: ComputerId::local(),
            status: ComputerStatus::Live,
        });
        assert_eq!(
            resolve(&wanted, &desk, false),
            Resolution::Open {
                computer: ComputerId::local(),
                key: "k-a".into()
            }
        );
        desk.observe(&Change::Replace {
            computer: ComputerId::local(),
            rows: vec![],
        });
        assert_eq!(
            resolve(&wanted, &desk, false),
            Resolution::Gone("claude/p/a".into()),
            "a current listing without it settles at once"
        );
        assert_eq!(
            gone_text("claude/p/a"),
            "This session is no longer available: claude/p/a"
        );
    }

    #[test]
    fn a_qt_seeded_state_withdraws_banners_qt_delivered() {
        let mut qt = Fixture::new();
        let token = qt
            .observe(vec![asking("claude/p/a", "approval", "r")])
            .raised[0]
            .token
            .clone();
        let saved = serde_json::to_vec(qt.tracker.state()).unwrap();
        let mut client = Fixture {
            tracker: Tracker::new(seed(None, Some(&saved))),
            notices: 0,
        };
        assert!(
            client
                .observe(vec![asking("claude/p/a", "approval", "r")])
                .raised
                .is_empty(),
            "nothing repeats after the switch"
        );
        assert_eq!(
            client.observe(vec![session("claude/p/a")]).cleared,
            vec![token]
        );
    }

    #[test]
    fn the_seed_prefers_the_clients_own_file() {
        let own = br#"{"": {"run:x": {"phase": "approval", "request": "r"}}}"#;
        let qt = br#"{"mac": {}}"#;
        assert!(seed(Some(own), Some(qt)).contains_key(""));
        assert!(seed(None, Some(qt)).contains_key("mac"));
        assert!(seed(None, Some(b"[1]")).is_empty());
        assert!(
            seed(Some(b"garbage"), Some(qt)).is_empty(),
            "the client never seeds twice"
        );
    }

    #[test]
    fn preferences_validate_like_qt() {
        let valid = json!({"conversation_id": "c", "muted": false, "revision": 2, "suppressed": {
            "reply_id": "rep", "attention_signature": "0".repeat(64), "optional_question_ids": ["a", "b"],
            "main_attention": {"run_id": "r", "phase": "approval", "request_id": "", "started_at": 5.0},
            "child_attention": {"w": {"phase": "input", "request_id": "q", "started_at": null}}}});
        assert!(valid_preferences(&valid, "c"));
        assert!(!valid_preferences(&valid, "other"));
        let mut unsorted = valid.clone();
        unsorted["suppressed"]["optional_question_ids"] = json!(["b", "a"]);
        assert!(
            !valid_preferences(&unsorted, "c"),
            "question IDs strictly increase"
        );
        let mut extra = valid.clone();
        extra["extra"] = json!(1);
        assert!(!valid_preferences(&extra, "c"), "exact keys");
        let mut timeless = valid.clone();
        timeless["suppressed"]["main_attention"]["started_at"] = Value::Null;
        assert!(
            !valid_preferences(&timeless, "c"),
            "an identity-less request needs its start time"
        );
        let mut working = valid.clone();
        working["suppressed"]["child_attention"]["w"]["phase"] = json!("working");
        assert!(!valid_preferences(&working, "c"));
    }

    #[test]
    fn request_identity_is_qt_compact_json() {
        assert_eq!(request_json("req-1", Some(5.0)), r#"["req-1",null]"#);
        assert_eq!(request_json("", Some(1761600000.0)), r#"["",1761600000]"#);
        assert_eq!(
            request_json("", Some(1761600000.25)),
            r#"["",1761600000.25]"#
        );
        assert_eq!(request_json("", Some(0.0)), r#"["",null]"#);
        assert_eq!(request_json("", None), r#"["",null]"#);
        assert_eq!(concise("  a\tb\n c ", 120), "a b c");
        assert_eq!(concise(&"x".repeat(50), 40).chars().count(), 40);
    }

    #[test]
    fn tokens_are_base64url_without_padding() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"{\"host\":\"\"}\xff\xfe"] {
            let text = base64url(bytes);
            assert!(!text.contains(['=', '+', '/']));
            assert_eq!(from_base64url(&text).as_deref(), Some(bytes));
        }
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn delivery_follows_the_mac_notifier() {
        // test_macattention.mm: waitsForPermissionAndHonorsWithdrawal
        let mut delivery = Delivery::default();
        assert_eq!(
            delivery.post("t1", "Title", "Body"),
            vec![DeliveryStep::CheckAuthorization]
        );
        assert!(
            delivery.post("t1", "Title", "Body").is_empty(),
            "posted once"
        );
        assert!(
            delivery.post("t2", "Title", "Two").is_empty(),
            "waits for the running check"
        );
        assert_eq!(
            delivery.withdraw("t2"),
            vec![DeliveryStep::Remove { token: "t2".into() }]
        );
        assert_eq!(
            delivery.authorized(true, None),
            vec![DeliveryStep::Add {
                token: "t1".into(),
                title: "Title".into(),
                body: "Body".into()
            }]
        );
        assert!(delivery.added("t1", None).is_empty());
        assert!(
            delivery.post("t1", "Title", "Body").is_empty(),
            "a posted token is not posted again"
        );
        assert_eq!(
            delivery.post("t3", "T", "B"),
            vec![DeliveryStep::CheckAuthorization]
        );
        assert_eq!(
            delivery.authorized(false, None),
            vec![DeliveryStep::Fail {
                token: "t3".into(),
                detail: DENIED.into()
            }]
        );
        assert_eq!(
            delivery.post("t4", "T", "B"),
            vec![DeliveryStep::CheckAuthorization]
        );
        assert_eq!(delivery.authorized(true, None).len(), 1);
        assert_eq!(
            delivery.added("t4", Some("disk full".into())),
            vec![DeliveryStep::Fail {
                token: "t4".into(),
                detail: "disk full".into()
            }]
        );
        let _ = delivery.withdraw("t1");
        assert_eq!(
            delivery.added("t1", None),
            vec![DeliveryStep::Remove { token: "t1".into() }]
        );
        assert_eq!(request_id("abc"), "zerus.attention.abc");
    }

    // Beyond the brief: Qt's cases for each rule the tests above leave open.

    /// A named change to a row.
    type Variant = (&'static str, fn(&mut ViewRow));

    fn with_baseline(mut row: ViewRow, suppressed: Value) -> ViewRow {
        row.notify.preferences = Some(Preferences {
            muted: false,
            suppressed,
        });
        row
    }

    fn titles(changes: &Changes) -> Vec<&str> {
        changes
            .raised
            .iter()
            .map(|notice| notice.title.as_str())
            .collect()
    }

    fn asking_child(name: &str, phase: &str, request: &str) -> ViewRow {
        let mut row = session(name);
        row.activity = "busy".to_owned();
        row.notify.children = vec![ChildFacts {
            id: "w1".into(),
            label: "Reviewer".into(),
            phase: phase.into(),
            request: request.into(),
            started: Some(5.0),
        }];
        row
    }

    fn optional(name: &str, ids: &[&str]) -> ViewRow {
        let mut row = session(name);
        row.notify.cmd = "codex".into();
        row.notify.optional_reported = true;
        row.notify.optional_count = ids.len() as i64;
        row.notify.optional_ids = ids.iter().map(|id| (*id).to_owned()).collect();
        row
    }

    #[test]
    fn the_reason_follows_qt_order() {
        // AttentionTracker.cpp:131-134
        let mut fixture = Fixture::new();
        let mut recovery = asking("claude/p/a", "error", "");
        recovery.notify.recovery_reason = "Provider\n  failed".into();
        recovery.notify.activity_detail = "ignored".into();
        let mut detail = asking("claude/p/b", "error", "");
        detail.notify.activity_detail = "Waiting\tfor the network".into();
        let bare = asking("claude/p/c", "error", "");
        let input = asking("claude/p/d", "input", "r");
        let bodies: Vec<String> = fixture
            .observe(vec![recovery, detail, bare, input])
            .raised
            .into_iter()
            .map(|notice| notice.body)
            .collect();
        assert_eq!(
            bodies,
            vec![
                "claude/p/a / arch\nProvider failed",
                "claude/p/b / arch\nWaiting for the network",
                "claude/p/c / arch\nAgent needs attention",
                "claude/p/d / arch\nInput needed",
            ]
        );
    }

    #[test]
    fn a_request_belongs_to_its_run_or_its_name_and_creation() {
        let mut fixture = Fixture::new();
        fixture.observe(vec![asking("claude/p/a", "approval", "r")]);
        let mut runless = asking("claude/p/b", "approval", "r");
        runless.run_id = None;
        fixture.observe(vec![asking("claude/p/a", "approval", "r"), runless]);
        let mut keys: Vec<&String> = fixture.tracker.state()[""]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        keys.sort();
        assert_eq!(keys, vec!["name:claude/p/b:100", "run:run-claude/p/a"]);
    }

    #[test]
    fn concise_counts_qt_units_and_never_splits_a_character() {
        assert_eq!(
            concise(&"x".repeat(40), 40),
            "x".repeat(40),
            "exactly the limit stays whole"
        );
        assert_eq!(
            concise("a\u{1}b", 10),
            "a b",
            "a control character becomes a space"
        );
        // An emoji is two UTF-16 units, as Qt's QString counts it.
        assert_eq!(
            concise(&"\u{1F600}".repeat(3), 5),
            "\u{1F600}\u{1F600}\u{2026}"
        );
        assert_eq!(
            concise(&"\u{1F600}".repeat(3), 4),
            "\u{1F600}\u{2026}",
            "a cut never splits a pair"
        );
        assert_eq!(concise(&"\u{1F600}".repeat(3), 6), "\u{1F600}".repeat(3));
    }

    #[test]
    fn a_token_needs_host_name_and_notice_and_at_most_8192_characters() {
        let token = |object: Value| encode_token(object.as_object().unwrap());
        let valid =
            json!({"host": "", "name": "claude/p/a", "notice": "n", "run": "r", "created": 1});
        assert!(target(&token(valid.clone())).is_some());
        for missing in ["host", "name", "notice"] {
            let mut partial = valid.clone();
            partial.as_object_mut().unwrap().remove(missing);
            assert_eq!(target(&token(partial)), None, "{missing}");
        }
        let mut numeric_host = valid.clone();
        numeric_host["host"] = json!(1);
        assert_eq!(target(&token(numeric_host)), None);
        let mut empty_name = valid.clone();
        empty_name["name"] = json!("");
        assert_eq!(target(&token(empty_name)), None);
        // Tokens around the bound: 8192 characters is the most a token has.
        let sized = |extra: usize| {
            let mut long = valid.clone();
            long["name"] = json!(format!("claude/p/a{}", "x".repeat(extra)));
            token(long)
        };
        let tokens: Vec<String> = (6040..6120).map(sized).collect();
        let longest = tokens
            .iter()
            .filter(|t| t.len() <= 8192)
            .max_by_key(|t| t.len())
            .unwrap();
        assert_eq!(longest.len(), 8192);
        assert!(target(longest).is_some());
        let over = tokens.iter().find(|t| t.len() > 8192).unwrap();
        assert_eq!(target(over), None);
        assert_eq!(
            target(&format!("{}==", token(valid))).map(|t| t["notice"].clone()),
            Some(json!("n")),
            "padding is accepted"
        );
    }

    #[test]
    fn a_click_on_a_peer_looks_at_that_peer() {
        let mut fixture = Fixture::new();
        let rows: BTreeMap<String, ViewRow> = [asking("claude/p/a", "approval", "r")]
            .into_iter()
            .map(|row| (row.name.clone(), row))
            .collect();
        let raised = fixture.tracker.observe(
            &Host {
                alias: "mac",
                label: "mac",
                ok: true,
                rows: &rows,
            },
            1000,
            false,
            &mut || "n".to_owned(),
        );
        let wanted = target(&raised.raised[0].token).unwrap();
        assert_eq!(wanted["host"], "mac");
        let mut desk = Desk::default();
        desk.observe(&Change::Replace {
            computer: ComputerId::local(),
            rows: vec![("k-a".into(), asking("claude/p/a", "approval", "r"))],
        });
        desk.observe(&Change::Computer {
            computer: ComputerId::from("mac"),
            status: ComputerStatus::Probing,
        });
        assert_eq!(
            resolve(&wanted, &desk, false),
            Resolution::Waiting,
            "this computer's row is not the peer's"
        );
        desk.observe(&Change::Replace {
            computer: ComputerId::from("mac"),
            rows: vec![("k-m".into(), asking("claude/p/a", "approval", "r"))],
        });
        assert_eq!(
            resolve(&wanted, &desk, false),
            Resolution::Open {
                computer: ComputerId::from("mac"),
                key: "k-m".into()
            }
        );
    }

    #[test]
    fn a_background_count_of_zero_is_idle() {
        let mut fixture = Fixture::new();
        let mut row = replied("claude/p/a", "rep-1");
        row.notify.background = 0;
        assert_eq!(titles(&fixture.observe(vec![row])), vec![NEW_REPLY]);
    }

    #[test]
    fn optional_questions_keep_64_seen_ids_and_name_the_newest() {
        let mut fixture = Fixture::new();
        fixture.tracker.set_optional_questions(true);
        let first = fixture.observe(vec![optional("codex/p/a", &["a", "b"])]);
        assert_eq!(
            target(&first.raised[0].token).unwrap()["question"],
            "b",
            "the newest fresh question"
        );
        let mut more = optional("codex/p/a", &["c"]);
        more.notify.optional_count = 5;
        let changes = fixture.observe(vec![more]);
        assert_eq!(
            changes.raised[0].body, "codex/p/a / arch\n5 optional questions waiting",
            "the host's whole count"
        );
        let ids: Vec<String> = (0..70).map(|i| format!("q{i:02}")).collect();
        let listed: Vec<&str> = ids.iter().map(String::as_str).collect();
        fixture.observe(vec![optional("codex/p/a", &listed[..60])]);
        fixture.observe(vec![optional("codex/p/a", &listed[60..])]);
        let seen = fixture.tracker.state()[""]["question:codex:conv-codex/p/a"]["seen"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(seen.len(), SEEN_QUESTIONS);
        // The oldest answered ones go first; the listed ones are always kept.
        assert_eq!(seen.first(), Some(&json!("q06")));
        assert_eq!(seen.last(), Some(&json!("q69")));
        assert!(!seen.contains(&json!("a")) && !seen.contains(&json!("q05")));
        assert_eq!(
            seen.iter().filter(|id| *id == "q60").count(),
            1,
            "no ID twice"
        );
    }

    #[test]
    fn a_baseline_withdraws_the_question_banner_it_names() {
        let mut fixture = Fixture::new();
        fixture.tracker.set_optional_questions(true);
        let token = fixture.observe(vec![optional("codex/p/a", &["q1"])]).raised[0]
            .token
            .clone();
        let names = |ids: Value| {
            json!({"reply_id": null, "attention_signature": null, "optional_question_ids": ids,
                   "main_attention": null, "child_attention": {}})
        };
        assert_eq!(
            fixture.observe(vec![with_baseline(
                optional("codex/p/a", &["q1"]),
                names(json!(["q0"]))
            )]),
            Changes::default()
        );
        let changes = fixture.observe(vec![with_baseline(
            optional("codex/p/a", &["q1"]),
            names(json!(["q1"])),
        )]);
        assert_eq!(
            changes,
            Changes {
                raised: vec![],
                cleared: vec![token]
            }
        );
        assert!(fixture
            .observe(vec![optional("codex/p/a", &["q1"])])
            .raised
            .is_empty());
    }

    #[test]
    fn only_a_live_row_makes_a_question_index_unknown() {
        let mut fixture = Fixture::new();
        fixture.tracker.set_optional_questions(true);
        let token = fixture.observe(vec![optional("codex/p/a", &["a"])]).raised[0]
            .token
            .clone();
        let mut stopped = optional("codex/p/old", &[]);
        stopped.conversation_id = Some("conv-codex/p/a".into());
        stopped.state = "stopped".into();
        stopped.notify.optional_count = -1;
        let changes = fixture.observe(vec![optional("codex/p/a", &[]), stopped]);
        assert_eq!(
            changes.cleared,
            vec![token],
            "a stopped row's unknown count does not keep the banner"
        );
    }

    #[test]
    fn an_offline_host_never_withdraws_for_an_archived_row_and_keeps_its_answer() {
        // muteWithdrawsOnlyMatchingOfflineBannersWithoutReplay
        let mut fixture = Fixture::new();
        let token = fixture
            .observe(vec![asking("claude/p/a", "approval", "r")])
            .raised[0]
            .token
            .clone();
        let mut archived = with_preferences(asking("claude/p/a", "approval", "r"), true);
        archived.state = "archived".into();
        assert_eq!(
            fixture.observe_at(vec![archived], 1000, false, false),
            Changes::default()
        );
        let mut replacement = with_preferences(asking("claude/p/a", "approval", "r"), true);
        replacement.run_id = Some("replacement".into());
        assert_eq!(
            fixture.observe_at(vec![replacement], 1000, false, false),
            Changes::default(),
            "another run's mute"
        );
        let before = fixture.tracker.state()[""]["run:run-claude/p/a"].clone();
        let muted = with_preferences(asking("claude/p/a", "approval", "r"), true);
        assert_eq!(
            fixture.observe_at(vec![muted], 1000, false, false).cleared,
            vec![token]
        );
        let mut consumed = before.as_object().unwrap().clone();
        consumed.remove("token");
        assert_eq!(
            fixture.tracker.state()[""]["run:run-claude/p/a"],
            Value::Object(consumed),
            "the identity stays"
        );
        let reply = fixture.observe(vec![replied("claude/p/r", "rep-1")]).raised[0]
            .token
            .clone();
        let muted_reply = with_preferences(replied("claude/p/r", "rep-1"), true);
        assert_eq!(
            fixture
                .observe_at(vec![muted_reply], 1000, false, false)
                .cleared,
            vec![reply],
            "by conversation"
        );
    }

    #[test]
    fn a_baseline_suppresses_only_its_own_phase() {
        let suppressed = json!({"reply_id": null, "attention_signature": null, "optional_question_ids": [],
            "main_attention": {"run_id": "run-claude/p/a", "phase": "input", "request_id": "r", "started_at": null},
            "child_attention": {"w1": {"phase": "input", "request_id": "q-1", "started_at": null}}});
        let changes = Fixture::new().observe(vec![
            with_baseline(asking("claude/p/a", "approval", "r"), suppressed.clone()),
            with_baseline(
                asking_child("claude/p/b", "approval", "q-1"),
                suppressed.clone(),
            ),
        ]);
        assert_eq!(
            changes.raised.len(),
            2,
            "the same request in another phase is new"
        );
        let quiet = Fixture::new().observe(vec![with_baseline(
            asking("claude/p/a", "input", "r"),
            suppressed,
        )]);
        assert!(quiet.raised.is_empty());
    }

    #[test]
    fn mute_consumes_every_edge_without_acknowledging() {
        // muteConsumesAllNotificationEdgesWithoutAcknowledging
        let mut row = asking("codex/p/muted", "approval", "approval-1");
        row.notify.cmd = "codex".into();
        row.activity = "busy".into();
        row.reply_id = Some("reply-1".into());
        row.notify.unread_reply = true;
        row.notify.subagent_source = "native".into();
        row.notify.children = vec![ChildFacts {
            id: "child".into(),
            label: "child".into(),
            phase: "input".into(),
            request: "child-1".into(),
            started: None,
        }];
        let mut fixture = Fixture::new();
        let changes = fixture.observe(vec![
            with_preferences(row.clone(), true),
            asking("codex/p/audible", "approval", "a"),
        ]);
        assert_eq!(changes.raised.len(), 1);
        assert!(changes.raised[0].body.starts_with("codex/p/audible"));
        let mut restarted = Fixture {
            tracker: Tracker::new(fixture.tracker.state().clone()),
            notices: 10,
        };
        assert!(restarted
            .observe(vec![with_preferences(row.clone(), false)])
            .raised
            .is_empty());
        row.phase = Some("idle".into());
        row.activity = "idle".into();
        row.notify.needs_action = false;
        row.notify.children.clear();
        assert!(
            restarted
                .observe(vec![with_preferences(row.clone(), false)])
                .raised
                .is_empty(),
            "the reply arrived while muted, before idleness"
        );
        row.reply_id = Some("reply-2".into());
        assert_eq!(
            titles(&restarted.observe(vec![with_preferences(row.clone(), false)])),
            vec![NEW_REPLY]
        );
        row.phase = Some("approval".into());
        row.notify.needs_action = true;
        row.notify.attention_id = "approval-2".into();
        assert_eq!(
            restarted
                .observe(vec![with_preferences(row, false)])
                .raised
                .len(),
            1
        );
    }

    #[test]
    fn a_baseline_question_is_never_announced() {
        // sharedOptionalBaselineDoesNotEvictNewObservedQuestions
        let mut fixture = Fixture::new();
        fixture.tracker.set_optional_questions(true);
        let names = json!({"reply_id": null, "attention_signature": null, "optional_question_ids": ["q1"],
            "main_attention": null, "child_attention": {}});
        let row = with_baseline(optional("codex/p/a", &["q1"]), names.clone());
        assert!(fixture.observe(vec![row]).raised.is_empty());
        assert_eq!(
            fixture
                .observe(vec![with_baseline(
                    optional("codex/p/a", &["q1", "q2"]),
                    names
                )])
                .raised
                .len(),
            1
        );
    }

    #[test]
    fn an_offline_mute_withdraws_a_reply_of_an_earlier_run() {
        let mut fixture = Fixture::new();
        let mut earlier = replied("claude/p/a", "rep-1");
        earlier.run_id = Some("earlier".into());
        let token = fixture.observe(vec![earlier]).raised[0].token.clone();
        let resumed = with_preferences(replied("claude/p/a", "rep-1"), true);
        assert_eq!(
            fixture
                .observe_at(vec![resumed], 1000, false, false)
                .cleared,
            vec![token],
            "by conversation, not run"
        );
    }

    #[test]
    fn a_native_request_identity_ignores_metadata_time_changes() {
        // nativeRequestIdentityIgnoresMetadataTimeChanges
        let mut fixture = Fixture::new();
        let mut row = asking("claude/p/a", "approval", "call-1");
        row.notify.attention_started = 10.5;
        assert_eq!(fixture.observe(vec![row.clone()]).raised.len(), 1);
        row.notify.attention_started = 20.5;
        assert!(fixture.observe(vec![row.clone()]).raised.is_empty());
        row.notify.attention_id = "call-2".into();
        assert_eq!(fixture.observe(vec![row]).raised.len(), 1);
    }

    #[test]
    fn a_baseline_keeps_a_raw_dsh_multi_request_identity() {
        // sharedSuppressionRetainsRawDshMultiRequestIdentity
        let request = "request-a\nrequest-b";
        let suppressed = json!({"reply_id": null, "attention_signature": null, "optional_question_ids": [],
            "main_attention": {"run_id": "run-dsh/p/a", "phase": "input", "request_id": request, "started_at": null},
            "child_attention": {}});
        let mut row = with_baseline(asking("dsh/p/a", "input", request), suppressed);
        row.notify.cmd = "dsh".into();
        let mut fixture = Fixture::new();
        assert!(fixture.observe(vec![row.clone()]).raised.is_empty());
        assert_eq!(
            fixture.tracker.state()[""]["run:run-dsh/p/a"]["request"],
            json!(format!("[{},null]", json!(request))),
            "the line feed stays in the identity"
        );
        row.notify.attention_id = "request-a\nrequest-c".into();
        assert_eq!(fixture.observe(vec![row]).raised.len(), 1);
    }

    #[test]
    fn an_offline_round_puts_the_entries_back_and_never_adds_a_host() {
        let mut fixture = Fixture::new();
        fixture.observe(vec![asking("claude/p/a", "approval", "r")]);
        let before = fixture.tracker.state().clone();
        assert_eq!(
            fixture.observe_at(
                vec![asking("claude/p/a", "approval", "r")],
                1000,
                false,
                false
            ),
            Changes::default()
        );
        assert_eq!(fixture.tracker.state(), &before);
        let rows = BTreeMap::new();
        fixture.tracker.observe(
            &Host {
                alias: "mac",
                label: "mac",
                ok: false,
                rows: &rows,
            },
            1000,
            false,
            &mut || "n".into(),
        );
        assert!(
            !fixture.tracker.state().contains_key("mac"),
            "a host never observed stays absent"
        );
        assert_eq!(
            integer(json!({"at": 9.0e15}).as_object().unwrap(), "at"),
            9_000_000_000_000_000
        );
        assert_eq!(
            integer(json!({"at": 1.0e16}).as_object().unwrap(), "at"),
            10_000_000_000_000_000,
            "Qt takes any whole double in the qint64 range"
        );
        assert_eq!(integer(json!({"at": 1.5}).as_object().unwrap(), "at"), 0);
        assert_eq!(integer(json!({"at": 1.0e19}).as_object().unwrap(), "at"), 0);
    }

    #[test]
    fn the_state_keeps_qt_keys_and_entries() {
        // A Qt-seeded state and this one must name the same edges.
        let mut fixture = Fixture::new();
        fixture.tracker.set_optional_questions(true);
        let mut row = asking_child("codex/p/a", "approval", "q-1");
        row.notify.cmd = "codex".into();
        row.reply_id = Some("rep-1".into());
        row.notify.unread_reply = true;
        row.activity = "idle".into();
        row.notify.optional_reported = true;
        row.notify.optional_count = 1;
        row.notify.optional_ids = vec!["oq-1".into()];
        let mut asking_row = asking("claude/p/b", "approval", "r-1");
        asking_row.run_id = None;
        fixture.observe(vec![row, asking_row]);
        let state = fixture.tracker.state()[""].as_object().unwrap();
        let mut keys: Vec<&str> = state.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "child:run:run-codex/p/a:w1",
                "name:claude/p/b:100",
                "question:codex:conv-codex/p/a",
                "reply:codex:conv-codex/p/a",
            ]
        );
        let entry = |key: &str| {
            let mut entry = state[key].as_object().unwrap().clone();
            assert!(
                entry
                    .remove("token")
                    .is_some_and(|token| target(token.as_str().unwrap()).is_some()),
                "{key}"
            );
            Value::Object(entry)
        };
        assert_eq!(
            entry("child:run:run-codex/p/a:w1"),
            json!({"phase": "approval", "request": "q-1"})
        );
        assert_eq!(
            entry("name:claude/p/b:100"),
            json!({"phase": "approval", "request": "r-1"})
        );
        assert_eq!(
            entry("reply:codex:conv-codex/p/a"),
            json!({"phase": "reply", "request": "rep-1"})
        );
        assert_eq!(
            entry("question:codex:conv-codex/p/a"),
            json!({"seen": ["oq-1"]})
        );
        let reply = target(
            state["reply:codex:conv-codex/p/a"]["token"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            Value::Object(reply),
            json!({"host": "", "name": "codex/p/a", "run": "run-codex/p/a", "created": 100,
            "notice": "notice-2", "reply": "rep-1", "conversation": "conv-codex/p/a", "agent": "codex"})
        );
    }

    #[test]
    fn a_scheduled_reply_with_an_unknown_schedule_count_waits() {
        let mut fixture = Fixture::new();
        let mut tick = replied("claude/p/a", "rep-1");
        tick.notify.reply_scheduled = true;
        tick.notify.recurring_schedules = -1;
        assert_eq!(fixture.observe(vec![tick.clone()]), Changes::default());
        assert!(
            fixture.tracker.state()[""].as_object().unwrap().is_empty(),
            "nothing is recorded either"
        );
        tick.notify.recurring_schedules = 0;
        assert_eq!(
            fixture.observe(vec![tick]).raised.len(),
            1,
            "the reply after the schedule ends"
        );
    }

    #[test]
    fn identity_less_requests_are_told_apart_by_their_start_time() {
        // backToBackIdentityLessRequestsEachAnnounce
        let mut fixture = Fixture::new();
        let mut prompt = asking("claude/p/a", "approval", "");
        prompt.notify.attention_started = 10.5;
        let first = fixture.observe(vec![prompt.clone()]);
        assert_eq!(first.raised.len(), 1);
        assert!(fixture.observe(vec![prompt.clone()]).raised.is_empty());
        prompt.notify.attention_started = 20.5;
        let next = fixture.observe(vec![prompt.clone()]);
        assert_eq!(next.raised.len(), 1);
        assert_eq!(next.cleared, vec![first.raised[0].token.clone()]);
        prompt.notify.acknowledged = true;
        assert_eq!(fixture.observe(vec![prompt.clone()]).cleared.len(), 1);
        prompt.notify.acknowledged = false;
        prompt.notify.attention_started = 30.5;
        assert_eq!(fixture.observe(vec![prompt.clone()]).raised.len(), 1);
        // State saved before start times were kept does not announce its
        // request again; the next one still notifies.
        let mut legacy = fixture.tracker.state().clone();
        for entry in legacy[""].as_object_mut().unwrap().values_mut() {
            entry.as_object_mut().unwrap().remove("started");
        }
        let mut updated = Fixture {
            tracker: Tracker::new(legacy),
            notices: 10,
        };
        assert!(updated.observe(vec![prompt.clone()]).raised.is_empty());
        prompt.notify.attention_started = 40.5;
        assert_eq!(updated.observe(vec![prompt]).raised.len(), 1);
    }

    #[test]
    fn a_whole_start_time_is_kept_as_qt_keeps_it() {
        let mut fixture = Fixture::new();
        let mut prompt = asking("claude/p/a", "approval", "");
        prompt.notify.attention_started = 10.0;
        fixture.observe(vec![prompt]);
        assert_eq!(
            fixture.tracker.state()[""]["run:run-claude/p/a"]["started"],
            json!(10)
        );
    }

    #[test]
    fn a_request_without_phase_or_identity_is_the_entry_qt_reads_for_a_new_key() {
        // Qt reads a missing entry as an empty one, which matches an empty
        // phase and request (AttentionTracker.cpp:92-98): no banner, as in Qt.
        let mut fixture = Fixture::new();
        let mut recovery = asking("claude/p/a", "", "");
        recovery.phase = None;
        assert!(fixture.observe(vec![recovery.clone()]).raised.is_empty());
        recovery.phase = Some("error".into());
        assert_eq!(fixture.observe(vec![recovery]).raised.len(), 1);
    }

    #[test]
    fn a_lapsed_acknowledgement_does_not_announce_the_same_request_again() {
        // lapsedAcknowledgementDoesNotAnnounceTheSameRequestAgain
        let mut fixture = Fixture::new();
        let mut row = asking("claude/p/a", "approval", "call-1");
        row.notify.children = vec![ChildFacts {
            id: "worker".into(),
            label: "Reviewer".into(),
            phase: "approval".into(),
            request: "child-1".into(),
            started: None,
        }];
        assert_eq!(fixture.observe(vec![row.clone()]).raised.len(), 2);
        row.notify.acknowledged = true;
        let changes = fixture.observe(vec![row.clone()]);
        assert!(changes.raised.is_empty());
        assert_eq!(changes.cleared.len(), 2);
        row.notify.acknowledged = false;
        assert_eq!(fixture.observe(vec![row.clone()]), Changes::default());
        let mut restarted = Fixture {
            tracker: Tracker::new(fixture.tracker.state().clone()),
            notices: 10,
        };
        assert!(restarted.observe(vec![row.clone()]).raised.is_empty());
        row.notify.attention_id = "call-2".into();
        row.notify.children[0].request = "child-2".into();
        assert_eq!(restarted.observe(vec![row]).raised.len(), 2);
    }

    #[test]
    fn a_reply_waits_for_every_kind_of_work_to_end() {
        // replyWaitsForExplicitIndependentResult, nativeChildCountersPreserveUnknownEvidence
        let not_idle: Vec<Variant> = vec![
            ("busy", |row| row.activity = "busy".into()),
            ("working phase", |row| row.phase = Some("working".into())),
            ("a background job", |row| row.notify.background = 1),
            ("an active subagent", |row| row.notify.subagents_active = 1),
            ("an unknown subagent count", |row| {
                row.notify.subagents_active = -1
            }),
            ("a coordination wait", |row| {
                row.notify.coordination_wait = true
            }),
            ("a request", |row| row.notify.needs_action = true),
        ];
        for (case, change) in not_idle {
            let mut fixture = Fixture::new();
            let mut row = replied("claude/p/a", "rep-1");
            change(&mut row);
            assert!(
                !titles(&fixture.observe(vec![row])).contains(&NEW_REPLY),
                "{case}"
            );
            assert_eq!(
                fixture
                    .observe(vec![replied("claude/p/a", "rep-1")])
                    .raised
                    .len(),
                1,
                "{case}"
            );
        }
        let mut fixture = Fixture::new();
        let mut unknown = replied("claude/p/a", "rep-1");
        unknown.notify.background = -1;
        assert_eq!(
            fixture.observe(vec![unknown]).raised.len(),
            1,
            "an unknown job count is no job"
        );
    }

    #[test]
    fn a_reply_in_progress_keeps_only_its_own_entry() {
        let mut fixture = Fixture::new();
        let token = fixture.observe(vec![replied("claude/p/a", "rep-1")]).raised[0]
            .token
            .clone();
        let mut busy = replied("claude/p/a", "rep-2");
        busy.activity = "busy".into();
        assert_eq!(
            fixture.observe(vec![busy]),
            Changes {
                raised: vec![],
                cleared: vec![token]
            }
        );
        let mut read = replied("claude/p/a", "rep-2");
        read.notify.unread_reply = false;
        assert!(fixture.observe(vec![read]).raised.is_empty());
        assert!(
            fixture.tracker.state()[""].as_object().unwrap().is_empty(),
            "a read reply leaves no entry"
        );
    }

    #[test]
    fn replies_belong_to_their_agent_and_conversation() {
        // repliesPersistByConversationAndNeverConfuseIdleWithUnread
        let mut fixture = Fixture::new();
        let token = fixture.observe(vec![replied("claude/p/a", "rep-1")]).raised[0]
            .token
            .clone();
        let mut renamed = replied("claude/p/renamed", "rep-1");
        renamed.conversation_id = Some("conv-claude/p/a".into());
        renamed.run_id = Some("resumed".into());
        assert_eq!(
            fixture.observe(vec![renamed.clone()]),
            Changes::default(),
            "a rename or resume is the same reply"
        );
        let mut other_agent = renamed.clone();
        other_agent.notify.cmd = "codex".into();
        let changes = fixture.observe(vec![other_agent]);
        assert_eq!((changes.raised.len(), changes.cleared), (1, vec![token]));
        let mut archived = replied("claude/p/a", "rep-9");
        archived.state = "archived".into();
        let mut runless = replied("claude/p/b", "rep-1");
        runless.conversation_id = None;
        let mut replyless = replied("claude/p/c", "");
        replyless.reply_id = None;
        let quiet = fixture.observe(vec![archived, runless, replyless]);
        assert!(quiet.raised.is_empty());
    }

    #[test]
    fn children_notify_only_while_their_session_runs() {
        // onlyExplicitChildRequestsNotify
        let quiet: Vec<Variant> = vec![
            ("paused", |row| row.state = "paused".into()),
            ("exited", |row| row.process_state = "exited".into()),
            ("unknown activity", |row| row.activity = "unknown".into()),
            ("an ended conversation at work", |row| {
                row.notify.conversation_state = "ended".into()
            }),
            ("hook profiles", |row| {
                row.notify.subagent_source = "hook_profiles".into()
            }),
            ("acknowledged", |row| row.notify.acknowledged = true),
        ];
        for (case, change) in quiet {
            let mut row = asking_child("claude/p/a", "approval", "q-1");
            change(&mut row);
            assert!(
                Fixture::new().observe(vec![row]).raised.is_empty(),
                "{case}"
            );
        }
        let mut ended = asking_child("claude/p/a", "input", "q-1");
        ended.notify.conversation_state = "ended".into();
        ended.activity = "idle".into();
        let changes = Fixture::new().observe(vec![ended]);
        assert_eq!(
            changes.raised.len(),
            1,
            "an ended conversation that is idle still asks"
        );
        assert_eq!(
            changes.raised[0].body,
            "claude/p/a / arch\nSubagent Reviewer: Input needed"
        );
        let mut fixture = Fixture::new();
        let attention = fixture.observe(vec![asking_child("claude/p/a", "attention", "q-1")]);
        assert_eq!(
            attention.raised[0].body,
            "claude/p/a / arch\nSubagent Reviewer: Input needed"
        );
        assert!(fixture
            .observe(vec![asking_child("claude/p/a", "attention", "q-1")])
            .raised
            .is_empty());
        assert_eq!(
            fixture
                .observe(vec![asking_child("claude/p/a", "attention", "q-2")])
                .raised
                .len(),
            1
        );
    }

    #[test]
    fn a_suppressed_baseline_is_recorded_silently_and_a_newer_edge_notifies() {
        // sharedUnmuteSuppressesEventsMissedWhileOffline
        let baseline = json!({"reply_id": "reply-old", "attention_signature": "a".repeat(64),
            "optional_question_ids": ["optional-old"],
            "main_attention": {"run_id": "run-codex/p/a", "phase": "approval", "request_id": "approval-old", "started_at": 10},
            "child_attention": {"child": {"phase": "input", "request_id": "child-old", "started_at": 11}}});
        let mut row = asking("codex/p/a", "approval", "approval-old");
        row.notify.cmd = "codex".into();
        row.notify.attention_started = 10.0;
        row.reply_id = Some("reply-old".into());
        row.notify.unread_reply = true;
        row.notify.optional_reported = true;
        row.notify.optional_count = 1;
        row.notify.optional_ids = vec!["optional-old".into()];
        row.notify.children = vec![ChildFacts {
            id: "child".into(),
            label: "child".into(),
            phase: "input".into(),
            request: "child-old".into(),
            started: Some(11.0),
        }];
        let mut row = with_baseline(row, baseline);
        let mut fixture = Fixture::new();
        fixture.tracker.set_optional_questions(true);
        let observe = |fixture: &mut Fixture, row: &ViewRow| fixture.observe(vec![row.clone()]);
        assert!(
            observe(&mut fixture, &row).raised.is_empty(),
            "this device never saw these events"
        );
        row.notify.attention_id = "approval-new".into();
        assert_eq!(titles(&observe(&mut fixture, &row)), vec![NEEDS_ATTENTION]);
        row.notify.attention_id = "approval-old".into();
        row.notify.children[0].request = "child-new".into();
        let child = observe(&mut fixture, &row);
        assert_eq!(child.raised.len(), 1);
        assert!(child.raised[0]
            .body
            .ends_with("Subagent child: Input needed"));
        row.phase = Some("idle".into());
        row.activity = "idle".into();
        row.notify.needs_action = false;
        row.notify.children.clear();
        row.notify.optional_count = 2;
        row.notify.optional_ids.push("optional-new".into());
        assert_eq!(
            titles(&observe(&mut fixture, &row)),
            vec![OPTIONAL_QUESTION]
        );
        row.notify.optional_count = 0;
        row.notify.optional_ids.clear();
        row.reply_id = Some("reply-new".into());
        assert_eq!(titles(&observe(&mut fixture, &row)), vec![NEW_REPLY]);
        let mut restarted = Fixture {
            tracker: Tracker::new(fixture.tracker.state().clone()),
            notices: 10,
        };
        restarted.tracker.set_optional_questions(true);
        assert!(observe(&mut restarted, &row).raised.is_empty());
    }

    #[test]
    fn a_baseline_keeps_timestamp_and_run_boundaries() {
        // sharedSuppressionKeepsTimestampAndRunBoundaries
        let suppressed = json!({"reply_id": null, "attention_signature": null, "optional_question_ids": [],
            "main_attention": {"run_id": "run-kimi/p/a", "phase": "input", "request_id": "", "started_at": 10},
            "child_attention": {"child": {"phase": "input", "request_id": "", "started_at": 12}}});
        let mut row = with_baseline(asking("kimi/p/a", "input", ""), suppressed);
        row.notify.attention_started = 10.0;
        let mut fixture = Fixture::new();
        assert!(fixture.observe(vec![row.clone()]).raised.is_empty());
        assert_eq!(
            fixture.tracker.state()[""]["run:run-kimi/p/a"]["request"],
            json!(r#"["",10]"#)
        );
        row.notify.attention_started = 11.0;
        assert_eq!(fixture.observe(vec![row.clone()]).raised.len(), 1);
        row.run_id = Some("replacement".into());
        row.notify.attention_started = 10.0;
        assert_eq!(fixture.observe(vec![row.clone()]).raised.len(), 1);
        row.phase = Some("idle".into());
        row.activity = "idle".into();
        row.notify.needs_action = false;
        row.notify.children = vec![ChildFacts {
            id: "child".into(),
            label: "child".into(),
            phase: "input".into(),
            request: String::new(),
            started: Some(12.0),
        }];
        assert!(fixture.observe(vec![row.clone()]).raised.is_empty());
        row.notify.children[0].started = Some(13.0);
        assert_eq!(fixture.observe(vec![row.clone()]).raised.len(), 1);
        row.notify.children[0].started = None;
        assert_eq!(
            fixture.observe(vec![row]).raised.len(),
            1,
            "a timestamp baseline needs a known start time"
        );
    }

    #[test]
    fn a_suppressed_reply_and_a_muted_reply_are_recorded_without_a_banner() {
        let mut fixture = Fixture::new();
        let suppressed = json!({"reply_id": "rep-1", "attention_signature": null, "optional_question_ids": [],
            "main_attention": null, "child_attention": {}});
        assert!(fixture
            .observe(vec![with_baseline(
                replied("claude/p/a", "rep-1"),
                suppressed
            )])
            .raised
            .is_empty());
        assert!(
            fixture
                .observe(vec![replied("claude/p/a", "rep-1")])
                .raised
                .is_empty(),
            "never announced later"
        );
        assert!(fixture
            .observe(vec![with_preferences(replied("claude/p/a", "rep-2"), true)])
            .raised
            .is_empty());
        assert!(fixture
            .observe(vec![replied("claude/p/a", "rep-2")])
            .raised
            .is_empty());
        assert_eq!(
            fixture
                .observe(vec![replied("claude/p/a", "rep-3")])
                .raised
                .len(),
            1
        );
    }

    #[test]
    fn optional_questions_follow_qt() {
        // optionalQuestionsNotifyOncePerQuestionWhenEnabled
        let mut fixture = Fixture::new();
        fixture.tracker.set_optional_questions(true);
        let first = fixture.observe(vec![optional("codex/p/a", &["a"])]);
        assert_eq!(
            first.raised[0].body,
            "codex/p/a / arch\nOptional question waiting"
        );
        assert!(fixture
            .observe(vec![optional("codex/p/a", &["a"])])
            .raised
            .is_empty());
        // Answering one keeps the banner; a new question replaces it.
        assert_eq!(
            fixture
                .observe(vec![optional("codex/p/a", &[])])
                .cleared
                .len(),
            1
        );
        let changes = fixture.observe(vec![optional("codex/p/a", &["a", "b"])]);
        assert_eq!(
            (changes.raised.len(), changes.cleared.len()),
            (1, 0),
            "only the unseen b is new"
        );
        // Turning banners off withdraws them and still records new questions.
        fixture.tracker.set_optional_questions(false);
        assert_eq!(
            fixture
                .observe(vec![optional("codex/p/a", &["a", "b", "c"])])
                .cleared
                .len(),
            1
        );
        fixture.tracker.set_optional_questions(true);
        assert!(fixture
            .observe(vec![optional("codex/p/a", &["a", "b", "c"])])
            .raised
            .is_empty());
        // An acknowledged or muted session withdraws its banner.
        assert_eq!(
            fixture
                .observe(vec![optional("codex/p/a", &["a", "b", "c", "d"])])
                .raised
                .len(),
            1
        );
        let mut read = optional("codex/p/a", &["a", "b", "c", "d"]);
        read.notify.acknowledged = true;
        assert_eq!(fixture.observe(vec![read]).cleared.len(), 1);
        let muted = with_preferences(optional("codex/p/a", &["a", "b", "c", "d", "e"]), true);
        assert!(fixture.observe(vec![muted]).raised.is_empty());
        // Focus records without a banner.
        assert!(fixture
            .observe_at(vec![optional("codex/p/a", &["f"])], 1000, true, true)
            .raised
            .is_empty());
        assert!(fixture
            .observe(vec![optional("codex/p/a", &["f"])])
            .raised
            .is_empty());
        // A stopped session has no pending questions.
        let mut stopped = optional("codex/p/a", &["g"]);
        stopped.state = "stopped".into();
        assert!(fixture.observe(vec![stopped]).raised.is_empty());
        let mut exited = optional("codex/p/a", &["g"]);
        exited.process_state = "exited".into();
        assert!(fixture.observe(vec![exited]).raised.is_empty());
        assert_eq!(
            fixture
                .observe(vec![optional("codex/p/a", &["g"])])
                .raised
                .len(),
            1
        );
    }

    #[test]
    fn optional_questions_survive_unknown_counts_and_a_resume() {
        // optionalQuestionsSurviveUnknownCountsAndResume
        let mut fixture = Fixture::new();
        fixture.tracker.set_optional_questions(true);
        assert_eq!(
            fixture
                .observe(vec![optional("codex/p/a", &["a"])])
                .raised
                .len(),
            1
        );
        let mut unknown = optional("codex/p/a", &[]);
        unknown.notify.optional_count = -1;
        assert_eq!(
            fixture.observe(vec![unknown.clone()]),
            Changes::default(),
            "an unreadable index keeps the banner"
        );
        let mut stopped = optional("codex/p/a", &["a"]);
        stopped.state = "stopped".into();
        assert_eq!(fixture.observe(vec![stopped.clone()]).cleared.len(), 1);
        let mut resumed = optional("codex/p/renamed", &["a"]);
        resumed.conversation_id = Some("conv-codex/p/a".into());
        assert!(fixture.observe(vec![resumed.clone()]).raised.is_empty());
        // A stopped instance of the conversation does not hide the live one.
        resumed.notify.optional_ids.push("b".into());
        resumed.notify.optional_count = 2;
        let changes = fixture.observe(vec![resumed.clone(), stopped.clone()]);
        assert_eq!(changes.raised.len(), 1);
        assert!(changes.raised[0].body.starts_with("codex/p/renamed / arch"));
        let destination = target(&changes.raised[0].token).unwrap();
        assert_eq!(destination["run"], "run-codex/p/renamed");
        assert!(
            matches(&destination, &resumed)
                && !matches(&destination, &{
                    let mut other = resumed.clone();
                    other.conversation_id = Some("other".into());
                    other
                })
        );
        // A muted unknown index withdraws the banner and keeps the IDs.
        let mut muted = with_preferences(resumed.clone(), true);
        muted.notify.optional_count = -1;
        assert_eq!(fixture.observe(vec![muted]).cleared.len(), 1);
        assert_eq!(
            fixture.tracker.state()[""]["question:codex:conv-codex/p/a"]["seen"],
            json!(["a", "b"])
        );
        // Hosts without the field add nothing.
        let mut older = optional("codex/p/z", &["z"]);
        older.notify.optional_reported = false;
        assert!(fixture.observe(vec![older]).raised.is_empty());
    }

    #[test]
    fn optional_questions_defer_to_requests_and_retry() {
        // optionalQuestionsDeferToRequestsAndRetry
        let mut fixture = Fixture::new();
        fixture.tracker.set_optional_questions(true);
        let mut asking_too = optional("codex/p/a", &["a"]);
        asking_too.phase = Some("input".into());
        asking_too.notify.needs_action = true;
        asking_too.notify.attention_id = "call-1".into();
        assert_eq!(
            titles(&fixture.observe(vec![asking_too])),
            vec![NEEDS_ATTENTION]
        );
        let answered = fixture.observe(vec![optional("codex/p/a", &["a"])]);
        assert!(
            answered.raised.is_empty(),
            "its questions were recorded meanwhile"
        );
        assert_eq!(answered.cleared.len(), 1);
        // Optional notices come first in one round, so a blocking request posts last.
        let mut waiting = optional("codex/p/other", &[]);
        waiting.phase = Some("approval".into());
        waiting.notify.needs_action = true;
        waiting.notify.attention_id = "call-2".into();
        let rows = || vec![optional("codex/p/a", &["a", "b"]), waiting.clone()];
        let both = fixture.observe(rows());
        assert_eq!(titles(&both), vec![OPTIONAL_QUESTION, NEEDS_ATTENTION]);
        fixture.tracker.delivery_failed(&both.raised[0].token, 1002);
        assert!(fixture
            .observe_at(rows(), 1061, false, true)
            .raised
            .is_empty());
        let retried = fixture.observe_at(rows(), 1062, false, true);
        assert_eq!(titles(&retried), vec![OPTIONAL_QUESTION]);
        assert_eq!(retried.cleared, vec![both.raised[0].token.clone()]);
        assert_eq!(
            target(&retried.raised[0].token).unwrap()["question"],
            "b",
            "the latest question when none is new"
        );
        assert!(fixture
            .observe_at(rows(), 1124, false, true)
            .raised
            .is_empty());
    }
}
