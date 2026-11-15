use super::*;

pub(super) const LIVE_PREDICATE: &str = "status IN ('\"planning\"','\"running\"','\"paused\"')";
pub(super) const WAITING_SQL: &str = "SELECT seq,kind,node,actor,text,state,decision,decided_actor,created_ms,decisions,notes,team,parameters FROM cards WHERE state='open' AND seq>?1 AND seq<=?2 ORDER BY seq LIMIT ?3";
pub(super) fn teams_sql(live: bool) -> String {
    let filter = if live {
        format!(" AND {LIVE_PREDICATE}")
    } else {
        String::new()
    };
    format!("SELECT id,project,name,status,lead,home,revision FROM teams WHERE id>?1 AND rowid<=?2{filter} ORDER BY id LIMIT ?3")
}
impl Store {
    /// Metadata only, ordered by ID with an insertion high-water mark.
    pub fn teams(&self, limit: u32, cursor: Option<&Cursor>) -> Result<Page<TeamRow>> {
        self.team_rows(false, limit, cursor)
    }
    /// The service pass uses the live-only index, independent of archive size.
    pub fn live_teams(&self, limit: u32, cursor: Option<&Cursor>) -> Result<Page<TeamRow>> {
        self.team_rows(true, limit, cursor)
    }
    fn team_rows(&self, live: bool, limit: u32, cursor: Option<&Cursor>) -> Result<Page<TeamRow>> {
        let scope = if live {
            "global:live_teams"
        } else {
            "global:teams"
        };
        let c = start(
            self,
            scope,
            limit,
            cursor,
            "SELECT COALESCE(MAX(rowid),0) FROM teams",
        )?;
        let mut rows = Vec::new();
        if let Some(db) = &self.db {
            let mut s = db.prepare(&teams_sql(live)).map_err(storage)?;
            rows = s
                .query_map(rusqlite::params![c.rank, c.ceiling, limit], team_row)
                .map_err(storage)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(storage)?;
        }
        Ok(pages::finish(rows, c, limit, |r| (0, r.id.clone())))
    }
    /// Every store's open owner cards, oldest sequence first.
    pub fn waiting(&self, limit: u32, cursor: Option<&Cursor>) -> Result<Page<WaitingCard>> {
        let c = start(
            self,
            "global:waiting",
            limit,
            cursor,
            "SELECT COALESCE(MAX(seq),0) FROM cards",
        )?;
        let mut rows = Vec::new();
        if let Some(db) = &self.db {
            let mut s = db.prepare(WAITING_SQL).map_err(storage)?;
            rows = s
                .query_map(rusqlite::params![c.after, c.ceiling, limit], |r| {
                    Ok(WaitingCard {
                        team: r.get(11)?,
                        card: rows::card_row(r)?,
                    })
                })
                .map_err(storage)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(storage)?;
        }
        Ok(pages::finish(rows, c, limit, |r| {
            (r.card.seq, String::new())
        }))
    }
}
fn start(
    store: &Store,
    scope: &str,
    limit: u32,
    cursor: Option<&Cursor>,
    ceiling_sql: &str,
) -> Result<Cursor> {
    store.active()?;
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(Problem::new(
            code::OVER_CAP,
            "Page limit must be between 1 and 200.",
        ));
    }
    if let Some(c) = cursor {
        if c.scope != scope || c.after > c.ceiling {
            return Err(Problem::new(
                code::INVALID_STATE,
                "Cursor belongs to a different list.",
            ));
        }
        return Ok(c.clone());
    }
    let ceiling = store.db.as_ref().map_or(Ok(0), |db| {
        db.query_row(ceiling_sql, [], |r| r.get(0)).map_err(storage)
    })?;
    Ok(Cursor {
        scope: scope.into(),
        ceiling,
        after: 0,
        rank: String::new(),
    })
}
fn team_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TeamRow> {
    Ok(TeamRow {
        id: r.get(0)?,
        project: r.get(1)?,
        name: r.get(2)?,
        status: sql_json(r, 3)?,
        lead: r.get(4)?,
        home: r.get(5)?,
        revision: r.get(6)?,
    })
}
