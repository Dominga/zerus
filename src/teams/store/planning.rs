//! Planned startup has no executable roster until initial publication.
use super::*;

pub(super) fn extend(db: &Connection) -> Result<()> {
    let present = |db: &Connection| -> Result<bool> {
        db.query_row("SELECT count(*)=2 FROM sqlite_master WHERE name IN ('planning_startup','teams_planning_live')", [], |r| r.get(0)).map_err(storage)
    };
    if present(db)? {
        return Ok(());
    }
    let own = db.is_autocommit();
    db.execute_batch(if own {
        "BEGIN IMMEDIATE;"
    } else {
        "SAVEPOINT planning_startup;"
    })
    .map_err(begin_error)?;
    let result = timers::fence(db).and_then(|()| {
        if present(db)? {
            return Ok(());
        }
        db.execute_batch(include_str!("planning.sql"))
            .map_err(storage)
    });
    let end = match (&result, own) {
        (Ok(()), true) => "COMMIT;",
        (Ok(()), false) => "RELEASE planning_startup;",
        (Err(_), true) => "ROLLBACK;",
        (Err(_), false) => "ROLLBACK TO planning_startup; RELEASE planning_startup;",
    };
    db.execute_batch(end).map_err(storage)?;
    result
}

pub(crate) fn paused(db: &Connection, team: &str) -> Result<Option<bool>> {
    db.query_row(
        "SELECT paused FROM planning_startup WHERE team=?1",
        [team],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Planning {
    pub paused: bool,
    pub awaiting_initial_approval: bool,
}

impl Store {
    pub fn planning(&self, team: &str) -> Result<Option<Planning>> {
        self.active()?;
        self.db.as_ref().map_or(Ok(None), |db| {
            paused(db, team).map(|paused| {
                paused.map(|paused| Planning {
                    paused,
                    awaiting_initial_approval: true,
                })
            })
        })
    }
    /// Missing startup state never grants a planning launch.
    pub(crate) fn launch_allowed(&self, team: &Team, member: &str) -> Result<bool> {
        Ok(match team.status {
            TeamStatus::Running => true,
            TeamStatus::Planning if member == team.lead => {
                self.planning(&team.id)?.is_some_and(|p| !p.paused)
            }
            _ => false,
        })
    }
}
