//! Data only. Package 8 must prove this exact run's liveness and idleness from
//! its fresh fleet row; this fact confers no authority to resume or send.
use super::{ports::Result, types::Binding};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::{path::Path, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::desktop::teams) struct ResumedBinding {
    pub run_id: String,
    pub conversation_id: String,
    pub session_id: String,
    /// Milliseconds since the Unix epoch, from the original CLI receipt.
    pub receipt_at: i64,
}
#[allow(dead_code)] // Package 8 consumes this data port after the merge.
pub(in crate::desktop::teams) fn resumed_binding(
    state: &Path,
    team: &str,
    member: &str,
    launch: &str,
) -> Result<Option<ResumedBinding>> {
    let path = state.join("teams/standing-v1/journal.sqlite3");
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(_) => (),
    }
    let path = crate::platform::canonical(state)
        .map_err(|e| e.to_string())?
        .join("teams/standing-v1/journal.sqlite3");
    if !crate::platform::verify_private(&path, crate::platform::Private::File)
        .map_err(|e| e.to_string())?
    {
        return Err("Standing-confirmation facts are not private".into());
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|e| e.to_string())?;
    db.busy_timeout(Duration::from_millis(100))
        .map_err(|e| e.to_string())?;
    let row:Option<(String,i64)>=db.query_row("SELECT body,receipt_ms FROM standing_bindings WHERE team=?1 AND member=?2 AND launch=?3 AND length(CAST(body AS BLOB))<=131072",params![team,member,launch],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
    let Some((raw, at)) = row else {
        return Ok(None);
    };
    let b: Binding = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if b.team != team
        || b.member != member
        || b.launch != launch
        || b.run.is_empty()
        || b.conversation.is_empty()
        || b.session.is_empty()
        || at < 0
    {
        return Err("The completed resume fact is invalid".into());
    }
    Ok(Some(ResumedBinding {
        run_id: b.run,
        conversation_id: b.conversation,
        session_id: b.session,
        receipt_at: at,
    }))
}
