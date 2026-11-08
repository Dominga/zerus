//! Task 134 (owner, 2026-11-07): team messages by pull. The text of a team
//! message (a hand-out, a stage hand-out, the lead's inbox, a starved notice,
//! a member's message) lives in the working tier, never only in a terminal
//! paste: the terminal gets a one-line doorbell under the message's own
//! request id, and `hgs team inbox` / `hgs team current` return the text and
//! record the read receipt. Zerus also puts unread text into the agent's next
//! turn itself (the turn-start hook's added context; `hooks.rs`), so delivery
//! never depends on the agent's discipline; that never marks a message read.
//!
//! This is docs/lore.md section 11's working tier in Zerus state (11.5: the
//! tables live in `teams.sqlite3`, the team data's own file): messages with
//! `queued`, `rung`, `read`, their expiry (read: `read_at` + 7 days; unread:
//! `at` + 30 days, with a `working_unread_expired` event) and a bounded
//! cleanup (at most 500 rows a pass). A team whose memory is RustCodeLore keeps
//! its team messages here too: `hgs` does not link the lore crate, and a team
//! message must not wait for a store that may be on another computer.
use super::model::{SessionRef, Team, Unread};
use super::store::Event;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS working_messages(
    id TEXT PRIMARY KEY, request_id TEXT NOT NULL UNIQUE, team TEXT NOT NULL, by TEXT NOT NULL,
    to_member TEXT NOT NULL, kind TEXT NOT NULL, urgency TEXT NOT NULL, text TEXT NOT NULL, digest TEXT NOT NULL,
    at REAL NOT NULL, state TEXT NOT NULL, rung_at REAL, rings INTEGER NOT NULL DEFAULT 0,
    safe_point TEXT NOT NULL DEFAULT '', injected_at REAL, read_at REAL, read_via TEXT NOT NULL DEFAULT '',
    read_request TEXT NOT NULL DEFAULT '', expires REAL NOT NULL);
    CREATE INDEX IF NOT EXISTS working_messages_to ON working_messages(team,to_member,state,at);
    CREATE INDEX IF NOT EXISTS working_messages_expires ON working_messages(expires);
    CREATE TABLE IF NOT EXISTS working_jobs(team TEXT NOT NULL, member TEXT NOT NULL, key TEXT NOT NULL,
    kind TEXT NOT NULL, what TEXT NOT NULL, command TEXT NOT NULL, started_at REAL NOT NULL, ended_at REAL,
    PRIMARY KEY(team, member, key));
    CREATE INDEX IF NOT EXISTS working_jobs_team ON working_jobs(team, ended_at, started_at);";

/// Task 278: ended job rows kept per team; older ones go.
pub(crate) const JOBS_KEPT: i64 = 500;

/// Task 278: a member's job started: its kind, what it is, its command and
/// start. A job already recorded keeps its row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn job_started(tx: &Transaction, team: &str, member: &str, key: &str, kind: &str, what: &str, command: &str, started_at: f64) -> anyhow::Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO working_jobs(team,member,key,kind,what,command,started_at,ended_at) VALUES(?1,?2,?3,?4,?5,?6,?7,NULL)",
        params![team, member, key, kind, super::secrets::redact_working(what), super::secrets::redact_working(command), started_at],
    )?;
    Ok(())
}

/// Task 278: a member's job ended (it is no longer listed).
pub(crate) fn job_ended(tx: &Transaction, team: &str, member: &str, key: &str, at: f64) -> anyhow::Result<()> {
    tx.execute("UPDATE working_jobs SET ended_at=?4 WHERE team=?1 AND member=?2 AND key=?3 AND ended_at IS NULL", params![team, member, key, at])?;
    Ok(())
}

/// Task 278: keeps at most `JOBS_KEPT` ended rows of the team.
pub(crate) fn prune_jobs(tx: &Transaction, team: &str) -> anyhow::Result<()> {
    tx.execute(
        "DELETE FROM working_jobs WHERE team=?1 AND ended_at IS NOT NULL AND rowid NOT IN
            (SELECT rowid FROM working_jobs WHERE team=?1 AND ended_at IS NOT NULL ORDER BY ended_at DESC LIMIT ?2)",
        params![team, JOBS_KEPT],
    )?;
    Ok(())
}

/// Task 278: the team's jobs, the running ones first, then the latest ended,
/// at most `limit`: what each member waits on, for the lead and its playbook.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn jobs(db: &Connection, team: &str, limit: i64) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut statement = db.prepare(
        "SELECT member,kind,what,command,started_at,ended_at FROM working_jobs WHERE team=?1
         ORDER BY ended_at IS NOT NULL, COALESCE(ended_at, started_at) DESC LIMIT ?2",
    )?;
    let rows = statement.query_map(params![team, limit], |row| {
        Ok(json!({"member":row.get::<_, String>(0)?,"kind":row.get::<_, String>(1)?,"what":row.get::<_, String>(2)?,
            "command":row.get::<_, String>(3)?,"started_at":row.get::<_, f64>(4)?,"ended_at":row.get::<_, Option<f64>>(5)?}))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A Zerus team message (a hand-out with its dependencies' reports may be
/// long); a member's own message keeps lore's 2,000 characters.
pub(crate) const TEXT_LIMIT: usize = 32 * 1024;
pub(crate) const MEMBER_TEXT_LIMIT: usize = 2_000;
/// docs/lore.md 11.3: `message_days` and `unread_days`.
pub(crate) const MESSAGE_DAYS: f64 = 7.0;
pub(crate) const UNREAD_DAYS: f64 = 30.0;
/// docs/lore.md 11.4: the cleanup deletes at most this many rows a pass.
pub(crate) const CLEANUP_ROWS: i64 = 500;
/// docs/lore.md 11.4: unread messages per recipient a member may add to.
pub(crate) const CAP_NORMAL: u32 = 50;
pub(crate) const CAP_URGENT: u32 = 5;
/// An unread message rings once more after this long (normal), then the lead
/// is told after the same time again.
pub(crate) const RING_AGAIN_SECONDS: f64 = 20.0 * 60.0;
/// docs/lore.md 11.11 `urgent_bound`: an urgent message the member has not
/// read in this time rings again, then the lead is told.
pub(crate) const URGENT_BOUND_SECONDS: f64 = 10.0 * 60.0;
/// At most this much unread text goes into one turn's added context.
pub(crate) const TURN_CONTEXT_BYTES: usize = 24 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Urgency {
    #[default]
    Normal,
    Urgent,
}

impl Urgency {
    fn as_str(self) -> &'static str {
        match self {
            Urgency::Normal => "normal",
            Urgency::Urgent => "urgent",
        }
    }
}

/// One stored message.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Message {
    pub id: String,
    pub request_id: String,
    pub team: String,
    pub by: String,
    pub to: String,
    pub kind: String,
    pub urgency: String,
    pub text: String,
    pub at: f64,
    pub state: String,
    pub rung_at: Option<f64>,
    pub rings: u32,
    pub safe_point: String,
    pub injected_at: Option<f64>,
    pub read_at: Option<f64>,
    pub read_via: String,
    pub read_request: String,
    pub expires: f64,
}

impl Message {
    pub(crate) fn urgent(&self) -> bool {
        self.urgency == "urgent"
    }
    /// What `hgs team inbox` and `current` show of it.
    pub(crate) fn value(&self) -> serde_json::Value {
        json!({"id":self.id,"kind":self.kind,"from":self.by,"urgency":self.urgency,"at":self.at,"state":self.state,
            "request_id":self.request_id,"text":self.text})
    }
}

const COLUMNS: &str = "id,request_id,team,by,to_member,kind,urgency,text,at,state,rung_at,rings,safe_point,injected_at,read_at,read_via,read_request,expires";

fn row(row: &rusqlite::Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        request_id: row.get(1)?,
        team: row.get(2)?,
        by: row.get(3)?,
        to: row.get(4)?,
        kind: row.get(5)?,
        urgency: row.get(6)?,
        text: row.get(7)?,
        at: row.get(8)?,
        state: row.get(9)?,
        rung_at: row.get(10)?,
        rings: row.get::<_, i64>(11)? as u32,
        safe_point: row.get(12)?,
        injected_at: row.get(13)?,
        read_at: row.get(14)?,
        read_via: row.get(15)?,
        read_request: row.get(16)?,
        expires: row.get(17)?,
    })
}

/// A store opened before this table existed reads as empty.
fn missing_table(error: &rusqlite::Error) -> bool {
    error.to_string().contains("no such table")
}

/// A new message: who writes it, to whom, what it is.
pub(crate) struct NewMessage<'a> {
    pub request_id: &'a str,
    pub by: &'a str,
    pub to: &'a str,
    pub kind: &'a str,
    pub urgency: Urgency,
    pub text: &'a str,
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Stores one message; the same request id answers the message it stored
/// (`false`: not new) and a request id reused for another text is refused.
/// Task 184: the text is working-tier text (docs/lore.md 11.9), so it is stored
/// redacted, never refused; the digest is of the redacted text, so a retry of
/// the same request stays idempotent.
pub(crate) fn post(tx: &Transaction, team: &str, new: &NewMessage, now: f64) -> anyhow::Result<(Message, bool)> {
    anyhow::ensure!(!new.request_id.is_empty() && !new.to.is_empty(), "A message names its request id and its recipient");
    anyhow::ensure!(!new.text.trim().is_empty(), "A message has text");
    anyhow::ensure!(
        new.text.len() <= TEXT_LIMIT,
        "A team message is at most {} KiB; this one is {} bytes",
        TEXT_LIMIT / 1024,
        new.text.len()
    );
    let text = super::secrets::redact_working(new.text);
    let wanted = digest(&text);
    let existing = tx
        .query_row(&format!("SELECT {COLUMNS} FROM working_messages WHERE request_id=?1"), params![new.request_id], row)
        .optional()?;
    if let Some(found) = existing {
        anyhow::ensure!(
            tx.query_row("SELECT digest FROM working_messages WHERE id=?1", params![found.id], |r| r.get::<_, String>(0))? == wanted
                && found.to == new.to,
            "request_id {} already names another message",
            new.request_id
        );
        return Ok((found, false));
    }
    let id = format!("w-message-{}", &digest(new.request_id)[..8]);
    tx.execute(
        "INSERT INTO working_messages(id,request_id,team,by,to_member,kind,urgency,text,digest,at,state,expires)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'queued',?11)",
        params![id, new.request_id, team, new.by, new.to, new.kind, new.urgency.as_str(), text, wanted, now, now + UNREAD_DAYS * 86_400.0],
    )?;
    let stored = tx.query_row(&format!("SELECT {COLUMNS} FROM working_messages WHERE id=?1"), params![id], row)?;
    Ok((stored, true))
}

/// A member's unread messages, oldest first (expired ones never show).
pub(crate) fn unread(db: &Connection, team: &str, member: &str, now: f64) -> anyhow::Result<Vec<Message>> {
    let mut statement = match db.prepare(&format!(
        "SELECT {COLUMNS} FROM working_messages WHERE team=?1 AND to_member=?2 AND state!='read' AND expires>?3 ORDER BY at, id LIMIT 200"
    )) {
        Ok(statement) => statement,
        Err(error) if missing_table(&error) => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let found = statement.query_map(params![team, member, now], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(found)
}

/// The message stored under a request id (a test reads what a doorbell stood for).
#[cfg(test)]
pub(crate) fn by_request(db: &Connection, request_id: &str) -> anyhow::Result<Option<Message>> {
    match db.query_row(&format!("SELECT {COLUMNS} FROM working_messages WHERE request_id=?1"), params![request_id], row).optional() {
        Ok(found) => Ok(found),
        Err(error) if missing_table(&error) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// A message whose doorbell was refused before anything reached the member,
/// and that never rang: gone, so the retry's own message is the only copy.
pub(crate) fn withdraw(tx: &Transaction, request_id: &str) -> anyhow::Result<()> {
    tx.execute("DELETE FROM working_messages WHERE request_id=?1 AND rings=0 AND injected_at IS NULL AND state!='read'", params![request_id])?;
    Ok(())
}

/// The doorbell rang for these messages (`safe_point`: how: idle, hook, turn_end).
pub(crate) fn rung(tx: &Transaction, ids: &[String], safe_point: &str, now: f64) -> anyhow::Result<()> {
    for id in ids {
        tx.execute(
            "UPDATE working_messages SET rings=rings+1, rung_at=?2, safe_point=?3,
             state=CASE WHEN state='queued' THEN 'rung' ELSE state END WHERE id=?1 AND state!='read'",
            params![id, now, safe_point],
        )?;
    }
    Ok(())
}

/// Zerus put these messages into a turn itself (no read mark).
pub(crate) fn injected(tx: &Transaction, ids: &[String], safe_point: &str, now: f64) -> anyhow::Result<()> {
    for id in ids {
        tx.execute(
            "UPDATE working_messages SET injected_at=?2, safe_point=CASE WHEN safe_point='' THEN ?3 ELSE safe_point END,
             state=CASE WHEN state='queued' THEN 'rung' ELSE state END WHERE id=?1 AND state!='read'",
            params![id, now, safe_point],
        )?;
    }
    Ok(())
}

/// The read receipt: only `hgs team inbox` and `hgs team current` give one.
/// Marks the named unread messages of `member` read; answers the ids marked.
pub(crate) fn mark_read(tx: &Transaction, team: &str, member: &str, ids: &[String], via: &str, request: &str, now: f64) -> anyhow::Result<Vec<String>> {
    let mut marked = Vec::new();
    for id in ids {
        let changed = tx.execute(
            "UPDATE working_messages SET state='read', read_at=?4, read_via=?5, read_request=?6, expires=?7
             WHERE id=?1 AND team=?2 AND to_member=?3 AND state!='read'",
            params![id, team, member, now, via, request, now + MESSAGE_DAYS * 86_400.0],
        )?;
        if changed > 0 {
            marked.push(id.clone());
        }
    }
    Ok(marked)
}

/// A member's unread summary for the team record (status, inbox, re-rings);
/// the lead's notice is kept while the oldest unread message is the same.
pub(crate) fn summary(db: &Connection, team: &str, member: &str, before: Option<&Unread>, now: f64) -> anyhow::Result<Option<Unread>> {
    let list = unread(db, team, member, now)?;
    let Some(oldest) = list.first() else { return Ok(None) };
    let lead_told = before.filter(|before| before.oldest == oldest.id).map(|before| before.lead_told).unwrap_or(false);
    Ok(Some(Unread {
        count: list.len() as u32,
        urgent: list.iter().filter(|message| message.urgent()).count() as u32,
        oldest: oldest.id.clone(),
        oldest_at: oldest.at,
        rings: oldest.rings,
        rung_at: oldest.rung_at,
        lead_told,
    }))
}

/// Refreshes `member`'s summary in the team record.
pub(crate) fn refresh(db: &Connection, team: &mut Team, member: &str, now: f64) -> anyhow::Result<()> {
    let team_id = team.id.clone();
    if let Some(found) = team.members.iter_mut().find(|found| found.id == member) {
        found.unread = summary(db, &team_id, member, found.unread.as_ref(), now)?;
    }
    Ok(())
}

/// One bounded cleanup pass: deletes at most `CLEANUP_ROWS` expired rows and
/// answers an event per unread message that expired unread (docs/lore.md 11.3).
pub(crate) fn cleanup(tx: &Transaction, team: &str, now: f64) -> anyhow::Result<Vec<Event>> {
    let mut statement = tx.prepare(&format!(
        "SELECT {COLUMNS} FROM working_messages WHERE team=?3 AND expires<=?1 ORDER BY expires LIMIT ?2"
    ))?;
    let expired = statement.query_map(params![now, CLEANUP_ROWS, team], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let mut events = Vec::new();
    for message in &expired {
        if message.state != "read" {
            // Task 184: the snippet is working-tier text still (11.9).
            let first: String = super::secrets::redact_working(&message.text).chars().take(80).collect();
            events.push(Event::new(
                "working_unread_expired",
                json!({"id":message.id,"from":message.by,"to":message.to,"at":message.at,"text":first}),
            ));
        }
        tx.execute("DELETE FROM working_messages WHERE id=?1", params![message.id])?;
    }
    Ok(events)
}

/// The doorbell: one line, never the text. `header` keeps the hand-out's own
/// first-line fields (`from`, `task`, `attempt`) when there are some.
pub(crate) fn doorbell(header: &str, count: usize, urgent: usize) -> String {
    let what = if count == 1 { "1 new message".to_owned() } else { format!("{count} new messages") };
    let urgency = if urgent > 0 { " \u{b7} URGENT" } else { "" };
    let header = if header.is_empty() { "[Zerus team".to_owned() } else { header.trim_end_matches(']').to_owned() };
    let line = format!("{header}{urgency}] {what} for you: hgs team inbox (or hgs team current)");
    line.chars().take(300).collect()
}

/// The first line of a team message when it is a `[Zerus team · ...]` header.
pub(crate) fn header_of(text: &str) -> &str {
    let first = text.lines().next().unwrap_or_default();
    if first.starts_with("[Zerus team") && first.ends_with(']') { first } else { "" }
}

/// What a turn receives of the unread messages (the hook's added context):
/// each message once, bounded; `None` when nothing is new for this turn.
pub(crate) fn turn_context(list: &[Message], only_urgent: bool) -> Option<(String, Vec<String>)> {
    let mut text = String::new();
    let mut ids = Vec::new();
    for message in list.iter().filter(|message| message.injected_at.is_none() && (!only_urgent || message.urgent())) {
        let part = format!(
            "[Zerus team message {} \u{b7} from {} \u{b7} {}]\n{}\n",
            message.id,
            message.by,
            if message.urgent() { "URGENT" } else { "normal" },
            message.text.trim_end()
        );
        if !text.is_empty() && text.len() + part.len() > TURN_CONTEXT_BYTES {
            break;
        }
        text.push_str(&part);
        ids.push(message.id.clone());
    }
    (!ids.is_empty()).then(|| {
        (
            format!(
                "{text}\nZerus put these unread team messages into this turn; they stay unread until you read them with hgs team inbox (or hgs team current)."
            ),
            ids,
        )
    })
}

/// The active team and member whose session runs as `name` with `run` on this
/// computer (the store's projection of team members). The conversation is
/// not compared: a compaction may have moved it on since the team recorded it.
pub(crate) fn session_member(store: &super::store::Store, session: &SessionRef) -> anyhow::Result<Option<(Team, String)>> {
    if session.name.is_empty() || session.run_id.is_empty() {
        return Ok(None);
    }
    let db = store.connection();
    let mut query = match db.prepare(
        "SELECT DISTINCT t.payload, m.member FROM teams t JOIN team_rule_members m ON t.id=m.team
         WHERE m.name=?1 AND m.run=?2 AND m.host=?3 AND t.status IN ('planning','running','paused') LIMIT 2",
    ) {
        Ok(query) => query,
        Err(error) if missing_table(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let rows = query
        .query_map(params![session.name, session.run_id, session.host], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    anyhow::ensure!(rows.len() <= 1, "Session belongs to more than one active team");
    let Some((payload, member)) = rows.into_iter().next() else { return Ok(None) };
    Ok(Some((serde_json::from_str(&payload)?, member)))
}

/// Task 134: what the turn-start hook adds to this session's turn: the
/// unread team messages Zerus has not put into a turn yet, each once. Marks
/// them injected (never read). None when the session is no team member's,
/// nothing is new, or the team store cannot be written from here (a
/// sandbox): the doorbell and `hgs team inbox` still carry the text.
pub(crate) fn turn_context_for(name: &str, run_id: &str) -> Option<String> {
    let path = super::store::path();
    if !path.exists() {
        return None;
    }
    let mut store = super::store::Store::open(true).ok()?;
    store.busy_timeout(std::time::Duration::from_secs(2)).ok()?;
    let session = SessionRef { name: name.to_owned(), run_id: run_id.to_owned(), ..Default::default() };
    let (team, member) = session_member(&store, &session).ok()??;
    let now = super::super::now();
    let list = unread(store.connection(), &team.id, &member, now).ok()?;
    let (text, ids) = turn_context(&list, false)?;
    store
        .mutate_working(&team.id, |tx, team, events| {
            injected(tx, &ids, "hook", now)?;
            refresh(tx, team, &member, now)?;
            events.push(Event::new("message_injected", json!({"to":team.role_of(&member),"ids":ids,"safe_point":"hook"})));
            Ok(())
        })
        .ok()?;
    Some(text)
}

#[cfg(test)]
#[path = "working_tests.rs"]
mod tests;
