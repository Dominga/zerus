//! Durable originals for Person task intake. These rows are evidence only:
//! no reader constructs a live permit or resumes a document-side write.
use super::*;
use crate::teams::ops;
use zerus_client_core::desktop::team_person_actions as wire;

const OBJECTS: &[(&str,&str,&str)]=&[
    ("person_action_originals","table","CREATE TABLE person_action_originals(
      request_id TEXT PRIMARY KEY NOT NULL CHECK(length(request_id)=36),
      team TEXT NOT NULL REFERENCES teams(id), task_project TEXT NOT NULL, task_uuid TEXT NOT NULL,
      intent TEXT NOT NULL CHECK(length(CAST(intent AS BLOB))<=65536),
      request TEXT NOT NULL CHECK(length(CAST(request AS BLOB))<=262144),
      state TEXT NOT NULL CHECK(state IN ('unknown','settled')),
      result TEXT CHECK(length(CAST(result AS BLOB))<=786432),
      CHECK((state='unknown' AND result IS NULL) OR (state='settled' AND result IS NOT NULL)))"),
    ("person_action_unsettled","index","CREATE INDEX person_action_unsettled ON person_action_originals(task_project,task_uuid,state,request_id) WHERE state='unknown'"),
    ("person_action_original_kept","trigger","CREATE TRIGGER person_action_original_kept BEFORE DELETE ON person_action_originals
      BEGIN SELECT RAISE(ABORT,'Native Person originals are kept'); END"),
    ("person_action_original_immutable","trigger","CREATE TRIGGER person_action_original_immutable BEFORE UPDATE ON person_action_originals
      WHEN NEW.request_id<>OLD.request_id OR NEW.team<>OLD.team OR NEW.task_project<>OLD.task_project OR NEW.task_uuid<>OLD.task_uuid
        OR NEW.intent<>OLD.intent OR NEW.request<>OLD.request OR OLD.state<>'unknown' OR NEW.state<>'settled' OR NEW.result IS NULL
      BEGIN SELECT RAISE(ABORT,'Native Person original identity and settled result are immutable'); END"),
];

fn unavailable() -> Problem {
    Problem::new(
        code::INVALID_STATE,
        "The exact native task original is unavailable. Nothing was replayed.",
    )
}
fn definitions(db: &Connection) -> Result<usize> {
    let mut found = 0;
    for (name, kind, expected) in OBJECTS {
        let actual:Option<(String,Option<String>)>=db.query_row(
            "SELECT type,CASE WHEN length(CAST(sql AS BLOB))<=2048 THEN sql END FROM sqlite_master WHERE name=?1",
            [name],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
        if let Some((actual_kind, actual_sql)) = actual {
            let normalize = |s: &str| s.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
            if actual_kind != *kind
                || actual_sql.as_deref().map(normalize).as_deref()
                    != Some(normalize(expected).as_str())
            {
                return Err(unavailable());
            }
            found += 1;
        }
    }
    Ok(found)
}
pub(crate) fn validate(db: &Connection) -> Result<()> {
    if definitions(db)? != OBJECTS.len() {
        return Err(unavailable());
    }
    Ok(())
}

/// The UUID reservation spans every operation actor. A missing leaf is only
/// pre-feature absence below the reserved core-7 floor; it is never repaired
/// or interpreted as no reservation in a store which requires that evidence.
pub(crate) fn reserved(db: &Connection, id: &str) -> Result<bool> {
    execution_fence(db)?;
    let present:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='person_action_originals')",[],|r|r.get(0)).map_err(storage)?;
    if !present {
        let version: u32 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage)?;
        return if version < 7 {
            Ok(false)
        } else {
            Err(unavailable())
        };
    }
    validate(db)?;
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM person_action_originals WHERE request_id=?1)",
        [id],
        |r| r.get(0),
    )
    .map_err(storage)
}
/// The core-7 migration calls this only for an entirely absent legacy leaf.
/// Current-layout readers must use validate, never reconstruct lost evidence.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    if db.is_autocommit() {
        return Err(unavailable());
    }
    if definitions(db)? == 0 {
        for (_, _, sql) in OBJECTS {
            db.execute_batch(sql).map_err(storage)?;
        }
    }
    validate(db)
}

pub(crate) struct Header {
    pub intent: wire::Intent,
    pub request: ops::Request,
    pub task: TaskRef,
}
impl Header {
    fn valid(&self) -> bool {
        self.intent.valid()
            && matches!(self.intent.store, wire::StoreBinding::Existing { .. })
            && self.intent.plan.request_id == self.request.request_id
            && self.intent.plan.team == self.request.team
            && self.intent.plan.expected_revision == self.request.expected_revision
            && wire::digest(&self.request).as_deref() == Some(&self.intent.request)
            && matches!(&self.request.operation,ops::Operation::AddTask {task,..} if task==&self.task)
            && !self.task.project.is_empty()
            && self.task.project.len() <= 128
            && !self.task.project.chars().any(char::is_control)
            && zerus_desktop_migration::canonical_uuid(&self.task.uuid)
    }
}
pub(crate) struct Original {
    pub intent: wire::Intent,
    pub request: ops::Request,
    pub task: TaskRef,
    /// None is explicitly unknown, never an effect-free refusal.
    pub result: Option<serde_json::Value>,
}

/// Only the original custodian calls this after consuming native admission
/// under this writer, and before any task document is changed.
pub(crate) fn reserve(db: &Connection, header: &Header) -> Result<()> {
    if db.is_autocommit() || !header.valid() {
        return Err(unavailable());
    }
    execution_fence(db)?;
    validate(db)?;
    let occupied:bool=db.query_row(
        "SELECT EXISTS(SELECT 1 FROM requests WHERE id=?1) OR EXISTS(SELECT 1 FROM person_action_originals WHERE request_id=?1)",
        [&header.request.request_id],|r|r.get(0)).map_err(storage)?;
    if occupied {
        return Err(Problem::new(
            code::DUPLICATE,
            "This original request UUID is already spent. Read its original result.",
        ));
    }
    let unknown:bool=db.query_row(
        "SELECT EXISTS(SELECT 1 FROM person_action_originals WHERE task_project=?1 AND task_uuid=?2 AND state='unknown')",
        params![header.task.project,header.task.uuid],|r|r.get(0)).map_err(storage)?;
    if unknown {
        return Err(unavailable());
    }
    db.execute("INSERT INTO person_action_originals(request_id,team,task_project,task_uuid,intent,request,state) VALUES(?1,?2,?3,?4,?5,?6,'unknown')",
        params![header.request.request_id,header.request.team,header.task.project,header.task.uuid,encode(&header.intent)?,encode(&header.request)?]).map_err(storage)?;
    Ok(())
}

type SavedRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

pub(crate) fn original(db: &Connection, intent: &wire::Intent) -> Result<Original> {
    if !intent.valid() {
        return Err(unavailable());
    }
    validate(db)?;
    let row:Option<SavedRow>=db.query_row(
        "SELECT team,task_project,task_uuid,intent,request,state,result FROM person_action_originals WHERE request_id=?1 AND length(CAST(intent AS BLOB))<=65536 AND length(CAST(request AS BLOB))<=262144 AND coalesce(length(CAST(result AS BLOB)),0)<=786432 AND length(CAST(team AS BLOB))<=36 AND length(CAST(task_project AS BLOB))<=128 AND length(CAST(task_uuid AS BLOB))<=36",
        [&intent.plan.request_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(storage)?;
    let (team, project, task, saved, request, state, result) = row.ok_or_else(unavailable)?;
    if saved != encode(intent)? {
        return Err(unavailable());
    }
    let header = Header {
        intent: intent.clone(),
        request: decode(&request)?,
        task: TaskRef {
            project,
            uuid: task,
        },
    };
    if !header.valid() || team != header.request.team {
        return Err(unavailable());
    }
    let result = match (state.as_str(), result) {
        ("unknown", None) => None,
        ("settled", Some(result)) => Some(decode(&result)?),
        _ => return Err(unavailable()),
    };
    Ok(Original {
        intent: header.intent,
        request: header.request,
        task: header.task,
        result,
    })
}

/// Settlement is one-way and belongs to the same live original. The caller
/// retains project custody through this transaction's actual commit.
pub(crate) fn settle(
    db: &Connection,
    intent: &wire::Intent,
    result: &serde_json::Value,
) -> Result<()> {
    if db.is_autocommit() {
        return Err(unavailable());
    }
    execution_fence(db)?;
    let saved = original(db, intent)?;
    if saved.result.is_some() {
        return Err(unavailable());
    }
    let body = encode(result)?;
    if body.len() > wire::BYTES {
        return Err(unavailable());
    }
    let changed=db.execute("UPDATE person_action_originals SET state='settled',result=?2 WHERE request_id=?1 AND state='unknown' AND intent=?3",
        params![intent.plan.request_id,body,encode(intent)?]).map_err(storage)?;
    if changed != 1 {
        return Err(unavailable());
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
