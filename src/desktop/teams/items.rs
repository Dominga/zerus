//! Named non-fatal conditions of the team service that have no card: a
//! stopped or archived team's cancel that keeps failing or has halted, a stop
//! cleanup whose key clears keep getting no answer, the terminal page's
//! fault. A live team's failing work has its card, and is never listed here.
//!
//! Each item mirrors one durable row of the service and is written and
//! cleared by that row's writer, in the transaction that writes or ends the
//! row; the service's bounded cleanup pass removes an item whose row is gone.
//! `hgs team status` reads one page of a team's items and the service-wide
//! ones read-only, with a capped count of the rest, and decides whether each
//! is current: the service records only when it last tried and when it tries
//! next.
use crate::teams::store::storage;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use zerus_team_plan::Problem;

type Result<T> = std::result::Result<T, Problem>;

/// The table, its row count and its cap, in the actor ledger. Additive: the
/// ledger's `user_version` is unchanged, so an older service still opens it.
pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS service_items (
 team TEXT NOT NULL, kind TEXT NOT NULL, subject TEXT NOT NULL, node INTEGER,
 reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB))<=512),
 failures INTEGER NOT NULL CHECK(failures>=0),
 since_ms INTEGER NOT NULL, last_ms INTEGER NOT NULL, next_ms INTEGER,
 PRIMARY KEY(team,kind,subject)
);
CREATE TABLE IF NOT EXISTS service_items_budget (id INTEGER PRIMARY KEY CHECK(id=1), rows INTEGER NOT NULL);
INSERT OR IGNORE INTO service_items_budget SELECT 1,COUNT(*) FROM service_items;
CREATE TRIGGER IF NOT EXISTS service_item_cap BEFORE INSERT ON service_items WHEN NEW.kind<>'unlisted' BEGIN
 SELECT CASE WHEN (SELECT rows>=16384 FROM service_items_budget WHERE id=1) THEN RAISE(ABORT,'service items full') END;
END;
CREATE TRIGGER IF NOT EXISTS service_item_added AFTER INSERT ON service_items BEGIN
 UPDATE service_items_budget SET rows=rows+1 WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS service_item_removed AFTER DELETE ON service_items BEGIN
 UPDATE service_items_budget SET rows=rows-1 WHERE id=1;
END;";

/// The kinds the service writes; a reader takes any string.
pub(crate) mod kind {
    /// A stopped or archived team's cancel that keeps failing; its subject
    /// is the decision.
    pub const CANCEL_RETRY: &str = "cancel_retry";
    /// A stopped or archived team's cancel whose refused outcome was refused
    /// too: it waits for the person; its subject is the decision.
    pub const CANCEL_HALTED: &str = "cancel_halted";
    /// A stopped or archived team's key clears that its last full sweep left
    /// without a final answer; no subject.
    pub const KEY_CLEAR: &str = "key_clear";
    /// The terminal page's fault, for the whole service.
    pub const TERMINAL_PAGE: &str = "terminal_page";
    /// A team whose project's own limits refuse every hand-out
    /// (zerus-63rru.23); the subject is empty, the team is the row's.
    pub const PROJECT_LIMIT: &str = "project_limit";
    /// A stopped or archived team's launch row whose failures have reached
    /// the retry cap: the team and what keeps failing, written once at the
    /// cap (zerus-63rru.24) and at once on the first pass after the stop
    /// (zerus-63rru.63); its subject is the member.
    pub const LAUNCH_FAILING: &str = "launch_failing";
}
/// The kind of the marker that says a team (or the service) has more items
/// than the table holds. It is never listed as an item.
pub(crate) const UNLISTED: &str = "unlisted";
/// Items in one status page.
pub(crate) const PAGE: usize = 20;
/// The count of the rest stops here and reads as "1000 or more".
pub(crate) const COUNT_CAP: usize = 1000;
/// An item is current while the service runs and its next try is at most
/// this late: two safety passes.
const GRACE_MS: i64 = 120_000;
/// The longest reason kept, in bytes.
pub(crate) const REASON_BYTES: usize = 512;

/// A reason as it may reach a person: whitespace and line breaks become one
/// space, every other control or invisible formatting character is removed,
/// and the text is cut at 512 bytes on a character boundary.
pub(crate) fn reason(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(REASON_BYTES));
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if c.is_control() || invisible(c) {
            continue;
        }
        let need = c.len_utf8() + usize::from(space);
        if out.len() + need > REASON_BYTES {
            break;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
    }
    out
}
/// Formatting characters that change how text reads without being seen:
/// zero-width characters, bidirectional controls and the byte-order mark.
fn invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}' | '\u{00AD}')
}

/// One failing subject, as its writer records it.
pub(crate) struct Failure<'a> {
    /// The team, or empty for a service-wide condition.
    pub team: &'a str,
    pub kind: &'a str,
    pub subject: &'a str,
    pub node: Option<u32>,
    pub reason: &'a str,
    pub failures: u32,
    pub now: i64,
    pub next_ms: Option<i64>,
}
/// Records or refreshes an item, inside the writer's transaction. Its first
/// time is kept; the reason is made safe here. When the table is full the
/// item is not listed, and the team's marker says so instead: the writer's
/// own row is never refused for its item.
pub(crate) fn put(db: &Connection, f: &Failure<'_>) -> Result<()> {
    let reason = reason(f.reason);
    // An existing item is refreshed in place: the cap's insert trigger would
    // refuse even an upsert of a row that is already counted.
    let refreshed = db
        .execute(
            "UPDATE service_items SET node=?4,reason=?5,failures=?6,last_ms=?7,next_ms=?8 WHERE team=?1 AND kind=?2 AND subject=?3",
            params![f.team, f.kind, f.subject, f.node, reason, f.failures, f.now, f.next_ms],
        )
        .map_err(storage)?;
    if refreshed > 0 {
        return Ok(());
    }
    let written = db.execute(
        "INSERT INTO service_items(team,kind,subject,node,reason,failures,since_ms,last_ms,next_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?7,?8)",
        params![f.team, f.kind, f.subject, f.node, reason, f.failures, f.now, f.next_ms],
    );
    match written {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(code, _))
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER =>
        {
            db.execute(
                "INSERT INTO service_items(team,kind,subject,node,reason,failures,since_ms,last_ms,next_ms) VALUES(?1,?2,'',NULL,'',0,?3,?3,NULL) ON CONFLICT(team,kind,subject) DO UPDATE SET last_ms=excluded.last_ms",
                params![f.team, UNLISTED, f.now],
            )
            .map_err(storage)?;
            Ok(())
        }
        Err(error) => Err(storage(error)),
    }
}
/// Removes an item, inside the transaction that ends its row.
pub(crate) fn clear(db: &Connection, team: &str, kind: &str, subject: &str) -> Result<()> {
    db.execute(
        "DELETE FROM service_items WHERE team=?1 AND kind=?2 AND subject=?3",
        params![team, kind, subject],
    )
    .map_err(storage)?;
    Ok(())
}
/// Removes every item of a team whose rows are reclaimed, its marker too.
pub(crate) fn clear_team(db: &Connection, team: &str) -> Result<()> {
    db.execute("DELETE FROM service_items WHERE team=?1", [team])
        .map_err(storage)?;
    Ok(())
}

/// The failures an item already counts, when it exists.
pub(crate) fn failures(
    db: &Connection,
    team: &str,
    kind: &str,
    subject: &str,
) -> Result<Option<u32>> {
    db.query_row(
        "SELECT failures FROM service_items WHERE team=?1 AND kind=?2 AND subject=?3",
        params![team, kind, subject],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)
}
/// An item's key, the orphan pass's cursor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Key {
    pub team: String,
    pub kind: String,
    pub subject: String,
}
/// One page of item keys after `after`, in key order.
pub(crate) fn keys(db: &Connection, after: &Key, limit: u32) -> Result<Vec<Key>> {
    let mut statement = db
        .prepare("SELECT team,kind,subject FROM service_items WHERE (team,kind,subject)>(?1,?2,?3) ORDER BY team,kind,subject LIMIT ?4")
        .map_err(storage)?;
    let rows = statement
        .query_map(params![after.team, after.kind, after.subject, limit], |r| {
            Ok(Key {
                team: r.get(0)?,
                kind: r.get(1)?,
                subject: r.get(2)?,
            })
        })
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage);
    rows
}
/// Whether the table has room for a new item: the orphan pass then removes
/// a team's marker, and its next failure is listed again.
pub(crate) fn has_room(db: &Connection) -> Result<bool> {
    db.query_row(
        "SELECT rows<16384 FROM service_items_budget WHERE id=1",
        [],
        |r| r.get(0),
    )
    .map_err(storage)
}

/// One item as `hgs team status` shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Item {
    /// `team`, or `service` for a condition of the whole service.
    pub scope: &'static str,
    pub kind: String,
    pub subject: String,
    pub node: Option<u32>,
    pub reason: String,
    pub failures: u32,
    pub since_ms: i64,
    pub last_ms: i64,
    pub next_ms: Option<i64>,
    /// The service runs and the next try is not overdue; otherwise the
    /// item is as of `last_ms`.
    pub current: bool,
}
/// One page of items and how many more there are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Items {
    pub items: Vec<Item>,
    /// Items after this page, counted up to [`COUNT_CAP`].
    pub more: usize,
    /// The table was full: some items of this team or of the service are
    /// not listed and not counted.
    pub unlisted: bool,
}

const PAGE_SQL: &str = "SELECT team,kind,subject,node,reason,failures,since_ms,last_ms,next_ms FROM service_items WHERE team IN ('',?1) AND kind<>'unlisted' ORDER BY team,kind,subject LIMIT ?2";
const COUNT_SQL: &str = "SELECT COUNT(*) FROM (SELECT 1 FROM service_items WHERE team IN ('',?1) AND kind<>'unlisted' LIMIT ?2)";
const UNLISTED_SQL: &str =
    "SELECT EXISTS(SELECT 1 FROM service_items WHERE team IN ('',?1) AND kind='unlisted')";

/// The items of `team` and of the service, from a ledger connection.
pub(crate) fn page(db: &Connection, team: &str, running: Option<bool>, now: i64) -> Result<Items> {
    let mut statement = db.prepare(PAGE_SQL).map_err(storage)?;
    let items = statement
        .query_map(params![team, PAGE as i64], |r| {
            let next_ms: Option<i64> = r.get(8)?;
            Ok(Item {
                scope: if r.get::<_, String>(0)?.is_empty() {
                    "service"
                } else {
                    "team"
                },
                kind: r.get(1)?,
                subject: r.get(2)?,
                node: r.get(3)?,
                reason: r.get(4)?,
                failures: r.get(5)?,
                since_ms: r.get(6)?,
                last_ms: r.get(7)?,
                next_ms,
                current: running == Some(true)
                    && next_ms.is_some_and(|next| now <= next.saturating_add(GRACE_MS)),
            })
        })
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)?;
    let counted: usize = db
        .query_row(COUNT_SQL, params![team, (PAGE + COUNT_CAP) as i64], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(storage)? as usize;
    let unlisted: bool = db
        .query_row(UNLISTED_SQL, [team], |r| r.get(0))
        .map_err(storage)?;
    Ok(Items {
        more: counted.saturating_sub(items.len()),
        items,
        unlisted,
    })
}

/// The actor ledger's connection for a writer outside the actor (the launch
/// worker): the same private file, opened for writing, with the items'
/// schema applied so the first writer creates the table. None when Teams is
/// off or the file cannot be opened safely.
fn write_connection(state: &Path) -> Option<Connection> {
    // The actor's own open: the same private file, the same schema and the
    // same migrations, so a writer outside the actor never forks them.
    crate::desktop::teams::ActorLedger::open(state)
        .ok()
        .map(|ledger| ledger.connection())
}
/// The launch writer's put: one named non-fatal item (zerus-63rru.24).
pub(crate) fn put_at(state: &Path, f: &Failure<'_>) -> Result<()> {
    let Some(db) = write_connection(state) else {
        return Err(Problem::new(
            zerus_team_plan::problem::code::STORAGE_FAILED,
            "The service items file cannot be opened.",
        ));
    };
    put(&db, f)
}
/// The launch writer's read of one item's count.
pub(crate) fn failures_at(state: &Path, team: &str, kind: &str, subject: &str) -> Result<Option<u32>> {
    let Some(db) = write_connection(state) else {
        return Err(Problem::new(
            zerus_team_plan::problem::code::STORAGE_FAILED,
            "The service items file cannot be opened.",
        ));
    };
    failures(&db, team, kind, subject)
}
/// The launch writer's clear: the row's successful pass ends its item.
pub(crate) fn clear_at(state: &Path, team: &str, kind: &str, subject: &str) -> Result<()> {
    let Some(db) = write_connection(state) else {
        return Err(Problem::new(
            zerus_team_plan::problem::code::STORAGE_FAILED,
            "The service items file cannot be opened.",
        ));
    };
    clear(&db, team, kind, subject)
}
/// The items `hgs team status` shows for `team`, read-only from the actor
/// ledger without creating it. None when Teams is off, the ledger or its
/// table does not exist (a service that does not write items), or the
/// ledger cannot be read safely.
pub(crate) fn read(state: &Path, team: &str, running: Option<bool>, now: i64) -> Option<Items> {
    use rusqlite::OpenFlags;
    if !crate::teams::enabled(state) {
        return None;
    }
    let state = crate::platform::canonical(state).ok()?;
    let directory = state.join("teams");
    if !crate::platform::verify_private(&directory, crate::platform::Private::Directory).ok()? {
        return None;
    }
    let path = directory.join("actor-v1.sqlite3");
    if !crate::platform::verify_private(&path, crate::platform::Private::File).ok()? {
        return None;
    }
    #[cfg(unix)]
    let identity = crate::sqlite_file::private(&path).ok()?;
    let db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .ok()?;
    #[cfg(unix)]
    crate::sqlite_file::bind_private(&db, &path, identity).ok()?;
    db.busy_timeout(std::time::Duration::from_millis(100))
        .ok()?;
    let present: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='service_items')",
            [],
            |r| r.get(0),
        )
        .optional()
        .ok()??;
    if !present {
        return None;
    }
    page(&db, team, running, now).ok()
}

#[cfg(test)]
mod tests;
