//! A fixed singleton independent of both bounded outboxes. No error text,
//! session identity or growing history is kept here.
use std::path::Path;
pub(super) const NOTIFICATION_OVERFLOW: &str = "launch_notification_overflow";
pub(super) const WORKER_FAULT: &str = "launch_worker_fault";
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Health {
    pub code: &'static str,
    pub at_ms: i64,
}
pub(super) fn read(state: &Path) -> Option<Health> {
    use rusqlite::{Connection, OpenFlags};
    if !crate::teams::enabled(state) {
        return None;
    }
    let state = crate::platform::canonical(state).ok()?;
    let directory = state.join("teams");
    if !crate::platform::verify_private(&directory, crate::platform::Private::Directory).ok()? {
        return None;
    }
    let path = directory.join(super::ledger::FILE);
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
    let schema: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).ok()?;
    if schema != super::ledger::SCHEMA {
        return None;
    }
    let (code, at_ms): (Option<i64>, Option<i64>) = db
        .query_row("SELECT code,at_ms FROM launch_health WHERE id=1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()?;
    let code = match code? {
        1 => NOTIFICATION_OVERFLOW,
        2 => WORKER_FAULT,
        _ => return None,
    };
    let at_ms = at_ms.filter(|t| *t >= 0)?;
    Some(Health { code, at_ms })
}
