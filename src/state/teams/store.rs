//! Durable team state. One private SQLite file holds every team
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
    use super::*;

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
