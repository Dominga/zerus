//! WhiteWh fork: durable team state. One private SQLite file holds every team
//! (whole JSON record with a revision), the feed (keyset pages) and the
//! idempotent operations, opened as the coordination waits are.
use super::super::*;
use super::model::Team;
use anyhow::{bail, ensure, Context};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::{fs, time::Duration};

/// Feed entries per page.
pub(crate) const PAGE: i64 = 50;

pub(crate) fn path() -> PathBuf {
    root().join("teams/teams.sqlite3")
}

/// One feed entry to append with a mutation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Event {
    pub kind: &'static str,
    pub payload: Value,
}

impl Event {
    pub(crate) fn new(kind: &'static str, payload: Value) -> Self {
        Self { kind, payload }
    }
}

pub(crate) struct Store {
    db: Connection,
}

/// The stable identity of a request's content, for idempotent replays.
pub(crate) fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

pub(crate) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

/// U20: a member's write the sandbox blocks says what to do, not a bare SQLite
/// error. Everything else is passed through unchanged.
pub(crate) fn write_error(error: impl std::fmt::Display) -> anyhow::Error {
    let text = error.to_string();
    if text.contains("readonly database") || text.contains("read-only database") {
        return anyhow::anyhow!(
            "the sandbox blocks the team store: ask for permission to run hgs team report (or ask/commit), then retry"
        );
    }
    anyhow::anyhow!(text)
}

/// A member in a sandbox runs at Low integrity (DeepSeek's confined shell),
/// which may not write files of the usual Medium label: its `hgs team
/// report` could not record the report. The team folder and what it holds
/// take the Low label once (what is created later inherits it); the folder
/// keeps its owner and permissions. Failing to set it leaves reports from
/// sandboxes to the member's last reply.
#[cfg(windows)]
fn sandbox_writable(dir: &Path) {
    use std::os::windows::process::CommandExt;
    let marker = dir.join(".sandbox-writable");
    if marker.exists() {
        return;
    }
    let done = std::process::Command::new("icacls")
        .arg(dir)
        .args(["/setintegritylevel", "(OI)(CI)L", "/T", "/C", "/Q"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .is_ok_and(|status| status.success());
    if done {
        let _ = fs::write(&marker, b"");
    }
}

impl Store {

    pub(crate) fn open(write: bool) -> anyhow::Result<Self> {
        Self::open_at(&path(), write)
    }

    pub(crate) fn open_at(path: &Path, write: bool) -> anyhow::Result<Self> {
        if let Ok(meta) = path.symlink_metadata() {
            ensure!(meta.is_file() && !meta.file_type().is_symlink(), "Nonregular team store");
        }
        if write {
            private_dir(path.parent().context("Missing team store parent")?).map_err(anyhow::Error::msg)?;
            if !path.exists() {
                crate::platform::private_open(fs::OpenOptions::new().write(true).create_new(true)).open(path)?;
            }
            #[cfg(windows)]
            sandbox_writable(path.parent().context("Missing team store parent")?);
        } else if !path.exists() {
            bail!("No teams yet");
        }
        let db = Connection::open_with_flags(
            path,
            if write { OpenFlags::SQLITE_OPEN_READ_WRITE } else { OpenFlags::SQLITE_OPEN_READ_ONLY },
        )?;
        db.busy_timeout(Duration::from_millis(2000))?;
        if write {
            db.execute_batch(
                "PRAGMA journal_mode=DELETE; PRAGMA max_page_count=262144;
                CREATE TABLE IF NOT EXISTS teams(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,status TEXT NOT NULL,
                    updated REAL NOT NULL,payload TEXT NOT NULL);
                CREATE INDEX IF NOT EXISTS teams_updated ON teams(status,updated);
                CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,team TEXT NOT NULL,at REAL NOT NULL,
                    kind TEXT NOT NULL,payload TEXT NOT NULL);
                CREATE INDEX IF NOT EXISTS events_team ON events(team,seq);
                CREATE INDEX IF NOT EXISTS events_team_kind_at ON events(team,kind,at);
                CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY,digest TEXT NOT NULL,receipt TEXT NOT NULL,at REAL NOT NULL);",
            )?;
        }
        Ok(Self { db })
    }

    pub(crate) fn load(&self, id: &str) -> anyhow::Result<Option<Team>> {
        let payload: Option<String> =
            self.db.query_row("SELECT payload FROM teams WHERE id=?1", params![id], |row| row.get(0)).optional()?;
        payload.map(|text| serde_json::from_str(&text).context("Invalid team record")).transpose()
    }

    /// Every team, most recently changed first, at most `limit`.
    pub(crate) fn teams(&self, include_archived: bool, limit: i64) -> anyhow::Result<Vec<Team>> {
        let mut statement = self.db.prepare(
            "SELECT payload FROM teams WHERE ?1 OR status!='archived' ORDER BY updated DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![include_archived, limit], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    /// The teams that still run: Planning, Running or Paused, most recently
    /// changed first, at most `limit`. Uses the (status, updated) index, so a
    /// finished team's status never reads every team (C1).
    pub(crate) fn active_teams(&self, limit: i64) -> anyhow::Result<Vec<Team>> {
        let mut statement = self.db.prepare(
            "SELECT payload FROM teams WHERE status IN ('planning','running','paused') ORDER BY updated DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    /// The sum of the active teams' revisions, without reading their payloads:
    /// U43's read-time default keys its cache by it, so a member added and one
    /// removed between two equal-length writes still refreshes the set.
    pub(crate) fn active_revision(&self) -> anyhow::Result<i64> {
        Ok(self.db.query_row(
            "SELECT COALESCE(SUM(revision),0) FROM teams WHERE status IN ('planning','running','paused')",
            [],
            |row| row.get(0),
        )?)
    }

    /// The stored receipt of an identical request, read only: a retry returns
    /// it before the request does any outside work (L4).
    pub(crate) fn receipt(&self, id: &str, input: &Value) -> anyhow::Result<Option<Value>> {
        ensure!(canonical_uuid(id), "A canonical request_id is required");
        let stored: Option<(String, String)> = self
            .db
            .query_row("SELECT digest,receipt FROM operations WHERE id=?1", params![id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        match stored {
            None => Ok(None),
            Some((digest_text, receipt)) => {
                ensure!(digest_text == digest(input), "This request_id was already used for another request");
                Ok(Some(serde_json::from_str(&receipt)?))
            }
        }
    }

    /// A page of one team's feed, newest first; `before` is the previous page's
    /// cursor. Entries appended meanwhile never shift an older page.
    pub(crate) fn events(&self, team: &str, before: Option<i64>, limit: i64) -> anyhow::Result<Value> {
        let limit = limit.clamp(1, PAGE);
        let mut statement = self.db.prepare(
            "SELECT seq,at,kind,payload FROM events WHERE team=?1 AND seq<?2 ORDER BY seq DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(params![team, before.unwrap_or(i64::MAX), limit + 1], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (seq, at, kind, payload) = row?;
            items.push(json!({"seq":seq,"at":at,"kind":kind,"detail":serde_json::from_str::<Value>(&payload)?}));
        }
        let more = items.len() as i64 > limit;
        items.truncate(limit as usize);
        let next = if more { items.last().map(|item| item["seq"].clone()) } else { None };
        Ok(json!({"items":items,"next_cursor":next}))
    }

    /// U38: one team's journal entries a timeline reads (spans::EVENT_KINDS)
    /// between `from` and `to`, oldest first, at most `limit`, through the
    /// (team, kind, at) index; true when more were there.
    pub(crate) fn span_events(&self, team: &str, from: f64, to: f64, limit: i64) -> anyhow::Result<(Vec<super::spans::Journal>, bool)> {
        let kinds = super::spans::EVENT_KINDS.iter().map(|kind| format!("'{kind}'")).collect::<Vec<_>>().join(",");
        let mut statement = self.db.prepare(&format!(
            "SELECT at,kind,payload FROM events WHERE team=?1 AND kind IN ({kinds}) AND at>=?2 AND at<=?3 ORDER BY at,seq LIMIT ?4"
        ))?;
        let rows = statement.query_map(params![team, from, to, limit.max(1) + 1], |row| {
            Ok((row.get::<_, f64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (at, kind, payload) = row?;
            items.push(super::spans::Journal { at, kind, detail: serde_json::from_str(&payload)? });
        }
        let cut = items.len() as i64 > limit.max(1);
        items.truncate(limit.max(1) as usize);
        Ok((items, cut))
    }

    /// Creates a team. A replay of the same request returns its first receipt.
    pub(crate) fn create(&mut self, request_id: &str, input: &Value, mut team: Team, events: Vec<Event>) -> anyhow::Result<Value> {
        let transaction = self.db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(receipt) = replay(&transaction, request_id, input)? {
            return Ok(receipt);
        }
        ensure!(
            transaction.query_row("SELECT 1 FROM teams WHERE id=?1", params![team.id], |_| Ok(())).optional()?.is_none(),
            "A team with this id already exists"
        );
        let at = now();
        team.revision = 1;
        team.created = at;
        team.updated = at;
        write(&transaction, &team)?;
        append(&transaction, &team.id, at, &events)?;
        let receipt = json!({"status":"created","request_id":request_id,"team":team.id,"revision":team.revision});
        record(&transaction, request_id, input, &receipt, at)?;
        transaction.commit()?;
        Ok(receipt)
    }

    /// Applies one change to a team in one transaction: checks the expected
    /// revision, bumps it and appends the change's feed entries. A request id
    /// makes it idempotent; the dispatcher's own changes pass none.
    /// #754: the receipt stored under a request id, whatever its input.
    pub(crate) fn stored_receipt(&self, id: &str) -> anyhow::Result<Option<Value>> {
        let stored: Option<String> = self
            .db
            .query_row("SELECT receipt FROM operations WHERE id=?1", params![id], |row| row.get(0))
            .optional()?;
        Ok(match stored {
            Some(text) => Some(serde_json::from_str(&text)?),
            None => None,
        })
    }

    pub(crate) fn mutate(
        &mut self,
        request: Option<(&str, &Value)>,
        team_id: &str,
        expected_revision: Option<u64>,
        change: impl FnOnce(&mut Team, &mut Vec<Event>) -> anyhow::Result<Value>,
    ) -> anyhow::Result<Value> {
        let transaction = self.db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some((id, input)) = request {
            if let Some(receipt) = replay(&transaction, id, input)? {
                return Ok(receipt);
            }
        }
        let payload: String = transaction
            .query_row("SELECT payload FROM teams WHERE id=?1", params![team_id], |row| row.get(0))
            .optional()?
            .context("Unknown team")?;
        let mut team: Team = serde_json::from_str(&payload).context("Invalid team record")?;
        if let Some(expected) = expected_revision {
            ensure!(team.revision == expected, "The team changed; refresh and try again");
        }
        let mut events = Vec::new();
        let mut receipt = change(&mut team, &mut events)?;
        let at = now();
        team.revision += 1;
        team.updated = at;
        write(&transaction, &team)?;
        append(&transaction, team_id, at, &events)?;
        if receipt.is_object() {
            receipt["team"] = json!(team_id);
            receipt["revision"] = json!(team.revision);
        }
        if let Some((id, input)) = request {
            if receipt.is_object() {
                receipt["request_id"] = json!(id);
            }
            record(&transaction, id, input, &receipt, at)?;
        }
        transaction.commit()?;
        Ok(receipt)
    }
}

fn write(transaction: &rusqlite::Transaction, team: &Team) -> anyhow::Result<()> {
    let status = serde_json::to_value(team.status)?;
    transaction.execute(
        "INSERT INTO teams(id,revision,status,updated,payload) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,status=excluded.status,updated=excluded.updated,payload=excluded.payload",
        params![team.id, team.revision as i64, status.as_str().unwrap_or_default(), team.updated, serde_json::to_string(team)?],
    )?;
    Ok(())
}

fn append(transaction: &rusqlite::Transaction, team: &str, at: f64, events: &[Event]) -> anyhow::Result<()> {
    for event in events {
        transaction.execute(
            "INSERT INTO events(team,at,kind,payload) VALUES(?1,?2,?3,?4)",
            params![team, at, event.kind, event.payload.to_string()],
        )?;
    }
    Ok(())
}

/// The stored receipt of an earlier identical request; another request that
/// reuses the id is refused.
fn replay(transaction: &rusqlite::Transaction, id: &str, input: &Value) -> anyhow::Result<Option<Value>> {
    ensure!(canonical_uuid(id), "A canonical request_id is required");
    let stored: Option<(String, String)> = transaction
        .query_row("SELECT digest,receipt FROM operations WHERE id=?1", params![id], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    match stored {
        None => Ok(None),
        Some((digest_text, receipt)) => {
            ensure!(digest_text == digest(input), "This request_id was already used for another request");
            Ok(Some(serde_json::from_str(&receipt)?))
        }
    }
}

fn record(transaction: &rusqlite::Transaction, id: &str, input: &Value, receipt: &Value, at: f64) -> anyhow::Result<()> {
    transaction.execute(
        "INSERT INTO operations(id,digest,receipt,at) VALUES(?1,?2,?3,?4)",
        params![id, digest(input), receipt.to_string(), at],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::model::{TeamStatus};
    use super::*;

    #[test]
    fn a_sandbox_blocked_write_says_what_to_do() {
        let error = write_error("attempt to write a readonly database").to_string();
        assert!(error.contains("the sandbox blocks the team store"), "{error}");
        assert!(error.contains("hgs team report"), "{error}");
        assert_eq!(write_error("disk I/O error").to_string(), "disk I/O error");
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        (dir, store)
    }
    fn team(id: &str) -> Team {
        Team { id: id.into(), name: "Checkout".into(), lead: "lead".into(), ..Default::default() }
    }
    const REQUEST: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn create_is_idempotent_and_a_reused_id_is_refused() {
        let (_dir, mut store) = store();
        let input = json!({"name":"Checkout"});
        let first = store.create(REQUEST, &input, team("t1"), vec![Event::new("created", json!({}))]).unwrap();
        assert_eq!(store.create(REQUEST, &input, team("t1"), vec![]).unwrap(), first);
        assert!(store.create(REQUEST, &json!({"name":"Other"}), team("t2"), vec![]).is_err());
        assert!(store.create("not-a-uuid", &input, team("t3"), vec![]).is_err());
        assert_eq!(store.load("t1").unwrap().unwrap().revision, 1);
    }

    #[test]
    fn the_active_revision_changes_with_a_team_change() {
        // #595 L2: the U43 default's cache key carries this revision, so a
        // change between two equal-length writes still refreshes the set.
        let (_dir, mut store) = store();
        store.create(REQUEST, &json!({}), team("t1"), vec![]).unwrap();
        let first = store.active_revision().unwrap();
        store
            .mutate(None, "t1", None, |team, _| {
                team.name = "Renamed".into();
                Ok(json!({}))
            })
            .unwrap();
        assert!(store.active_revision().unwrap() > first);
    }

    #[test]
    fn the_active_teams_query_is_bounded() {
        let (_dir, mut store) = store();
        for n in 0..12u32 {
            let mut t = team(&format!("t{n}"));
            t.status = if n % 4 == 0 { TeamStatus::Running } else { TeamStatus::Done };
            let request = format!("00000000-0000-4000-8000-{n:012}");
            store.create(&request, &json!({}), t, vec![]).unwrap();
        }
        // Only the three running teams, never the nine done ones, and at most
        // the requested limit (C1).
        let active = store.active_teams(1000).unwrap();
        assert_eq!(active.len(), 3);
        assert!(active.iter().all(|team| team.status == TeamStatus::Running));
        assert_eq!(store.active_teams(2).unwrap().len(), 2);
    }

    #[test]
    fn a_receipt_is_read_before_the_request_runs() {
        let (_dir, mut store) = store();
        store.create(REQUEST, &json!({"name":"Checkout"}), team("t1"), vec![]).unwrap();
        assert_eq!(store.receipt(REQUEST, &json!({"name":"Checkout"})).unwrap().unwrap()["status"], "created");
        assert!(store.receipt("22222222-2222-4222-8222-222222222222", &json!({})).unwrap().is_none());
        // The same id with another request is refused, as a mutation would.
        assert!(store.receipt(REQUEST, &json!({"name":"Other"})).is_err());
    }

    #[test]
    fn an_extend_replay_returns_the_first_receipt() {
        use super::super::model::{Member, TeamStatus};
        use super::super::ops::{self, Actor};
        use std::collections::BTreeSet;
        let (_dir, mut store) = store();
        let mut team = team("t1");
        team.status = TeamStatus::Running;
        team.members = vec![
            Member { id: "lead".into(), agent: "claude".into(), ..Default::default() },
            Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() },
        ];
        store.create(REQUEST, &json!({}), team, vec![]).unwrap();
        const EXTEND: &str = "22222222-2222-4222-8222-222222222222";
        let input = json!({"tasks":[{"id":1,"title":"Fix","assignee":"dev"}]});
        let extend = |store: &mut Store, input: &Value| {
            store.mutate(Some((EXTEND, input)), "t1", None, |team, events| {
                ops::apply(team, "extend", input, &Actor::Member("lead".into()), &BTreeSet::new(), events)
            })
        };
        let first = extend(&mut store, &input).unwrap();
        assert_eq!(first["status"], "added");
        let again = extend(&mut store, &input).unwrap();
        assert_eq!(again, first);
        assert_eq!(again["revision"], json!(2));
        assert_eq!(store.load("t1").unwrap().unwrap().phases.len(), 1);
        assert!(extend(&mut store, &json!({"tasks":[{"id":1,"title":"Other","assignee":"dev"}]})).is_err());
    }

    #[test]
    fn the_new_attempt_fields_survive_the_store_and_a_replay() {
        use super::super::model::{
            Attempt, AttemptState, Hold, Member, ReplacedReport, Report, Settings, Task, TaskStatus, TeamStatus,
        };
        use super::super::ops::{self, Actor};
        use std::collections::BTreeSet;
        let (_dir, mut store) = store();
        let mut stored = team("t1");
        stored.status = TeamStatus::Running;
        stored.settings = Settings { start_timeout_seconds: 300, ..Default::default() };
        stored.members = vec![
            Member { id: "lead".into(), agent: "claude".into(), ..Default::default() },
            Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() },
        ];
        stored.tasks = vec![
            Task {
                id: 1,
                title: "API".into(),
                assignee: "dev".into(),
                status: TaskStatus::InProgress,
                attempts: vec![
                    Attempt {
                        n: 1,
                        id: "00000000-0000-4000-8000-000000000001".into(),
                        member: "dev".into(),
                        state: AttemptState::Reported,
                        sent_at: 1.0,
                        delivered_at: Some(2.0),
                        started_at: Some(3.0),
                        start_warned: true,
                        turn_ended_at: Some(4.0),
                        turn_reply: "reply-7".into(),
                        report: Some(Report {
                            text: "Done".into(),
                            at: 6.0,
                            captured: true,
                            replaced: vec![ReplacedReport { text: "Building".into(), at: 5.5 }],
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    Attempt {
                        n: 2,
                        id: "00000000-0000-4000-8000-000000000002".into(),
                        member: "dev".into(),
                        state: AttemptState::Delivered,
                        sent_at: 8.0,
                        delivered_at: Some(9.0),
                        started_at: Some(10.0),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            Task {
                id: 2,
                title: "Tests".into(),
                assignee: "dev".into(),
                status: TaskStatus::Pending,
                hold: Some(Hold { by: "Lead".into(), at: 7.0, reason: "Busy with #1".into() }),
                ..Default::default()
            },
        ];
        store.create(REQUEST, &json!({}), stored.clone(), vec![]).unwrap();
        let loaded = store.load("t1").unwrap().unwrap();
        assert_eq!(loaded.tasks, stored.tasks);
        assert_eq!(loaded.settings.start_timeout_seconds, 300);
        assert_eq!(loaded.tasks[1].hold.as_ref().map(|hold| hold.reason.as_str()), Some("Busy with #1"));

        // A member's question, replayed with the same request: one receipt.
        const ASK: &str = "33333333-3333-4333-8333-333333333333";
        let input = json!({"text":"Must I keep the old JSON field?"});
        let ask = |store: &mut Store, input: &Value| {
            store.mutate(Some((ASK, input)), "t1", None, |team, events| {
                ops::apply(team, "ask", input, &Actor::Member("dev".into()), &BTreeSet::new(), events)
            })
        };
        let first = ask(&mut store, &input).unwrap();
        assert_eq!((first["status"].as_str(), first["question"].as_str()), (Some("asked"), Some("q1")));
        assert_eq!((first["task"].as_u64(), first["attempt"].as_u64()), (Some(1), Some(2)));
        assert_eq!(ask(&mut store, &input).unwrap(), first);
        let loaded = store.load("t1").unwrap().unwrap();
        let questions = &loaded.tasks[0].attempts[1].questions;
        assert_eq!(questions.len(), 1);
        assert_eq!(
            (questions[0].id.as_str(), questions[0].text.as_str(), questions[0].answered_at),
            ("q1", "Must I keep the old JSON field?", None)
        );
        // The feed has the question once, not once per replay.
        let feed = store.events("t1", None, PAGE).unwrap();
        let asked: Vec<&Value> =
            feed["items"].as_array().unwrap().iter().filter(|item| item["kind"] == "question_asked").collect();
        assert_eq!(asked.len(), 1);
        assert!(asked[0]["seq"].as_i64().is_some());
        assert_eq!(asked[0]["detail"]["question"], json!("q1"));
        assert_eq!(loaded.revision, 2);
    }

    #[test]
    fn a_mutation_checks_the_revision_and_appends_its_events() {
        let (_dir, mut store) = store();
        store.create(REQUEST, &json!({}), team("t1"), vec![]).unwrap();
        let changed = store
            .mutate(None, "t1", Some(1), |team, events| {
                team.goal = "Ship".into();
                events.push(Event::new("planned", json!({"tasks":0})));
                Ok(json!({"status":"planned"}))
            })
            .unwrap();
        assert_eq!(changed["revision"], 2);
        assert!(store.mutate(None, "t1", Some(1), |_, _| Ok(json!({}))).is_err());
        // A failed change leaves the team untouched.
        assert!(store.mutate(None, "t1", None, |team, _| { team.goal = "Lost".into(); bail!("refused") }).is_err());
        assert_eq!(store.load("t1").unwrap().unwrap().goal, "Ship");
    }

    #[test]
    fn feed_pages_stay_stable_while_entries_arrive() {
        let (_dir, mut store) = store();
        store.create(REQUEST, &json!({}), team("t1"), vec![]).unwrap();
        for n in 0..60 {
            store.mutate(None, "t1", None, |_, events| { events.push(Event::new("tick", json!({"n":n}))); Ok(json!({})) }).unwrap();
        }
        let first = store.events("t1", None, PAGE).unwrap();
        assert_eq!(first["items"].as_array().unwrap().len(), 50);
        assert_eq!(first["items"][0]["detail"]["n"], 59);
        let cursor = first["next_cursor"].as_i64().unwrap();
        store.mutate(None, "t1", None, |_, events| { events.push(Event::new("tick", json!({"n":60}))); Ok(json!({})) }).unwrap();
        let second = store.events("t1", Some(cursor), PAGE).unwrap();
        assert_eq!(second["items"].as_array().unwrap().len(), 10);
        assert_eq!(second["items"][0]["detail"]["n"], 9);
        assert!(second["next_cursor"].is_null());
    }
}
