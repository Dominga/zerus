//! Indexed base reads. A missing pin never adopts the team's newer default.
use super::*;

/// Reserved writer-fence namespace, independent of the ordinary layout sequence.
/// Its low bits retain the layout so later migrations still run and keep the fence.
pub(crate) const PINNED_LAYOUT_FLAG: u32 = versions::PINNED_LAYOUT_FLAG;
pub(crate) const PINNED_EXTENSION: u32 = versions::PINNED_EXTENSION;

// The capability name is stable; its suffix is not the layout number.
pub(crate) fn register_writer(db: &Connection) -> Result<()> {
    use rusqlite::functions::FunctionFlags;
    db.create_scalar_function(
        "zerus_teams_base_pins_v13",
        0,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |_| Ok(i64::from(PINNED_EXTENSION)),
    )
    .map_err(storage)
}

pub(crate) fn protect_writers(db: &Connection) -> Result<()> {
    timers::fence(db)?;
    db.execute_batch(include_str!("base-fence.sql"))
        .map_err(storage)?;
    db.execute(
        "UPDATE timers_extension SET version=?1 WHERE id=1",
        [PINNED_EXTENSION],
    )
    .map_err(storage)?;
    Ok(())
}

/// Older effects inherit the frozen pre-advance base only below its recorded
/// high-water. A newer missing pin fails closed. All joins are primary keys.
pub(crate) const VALUE: &str = "COALESCE(b.base_commit,CASE WHEN o.team IS NULL THEN t.base_commit WHEN e.seq<=o.legacy_ceiling THEN o.base_commit END)";
pub(crate) const JOINS: &str = "LEFT JOIN handout_bases b ON b.effect=e.request_id JOIN teams t ON t.id=e.team LEFT JOIN base_origins o ON o.team=e.team";

pub(crate) fn attempt(
    db: &Connection,
    team: &str,
    node: u32,
    generation: u32,
) -> Result<Option<String>> {
    db.query_row(&format!("SELECT {VALUE} FROM effects e {JOINS} WHERE e.team=?1 AND e.node=?2 AND e.generation=?3 AND e.state NOT IN ('refused','superseded') ORDER BY e.seq DESC LIMIT 1"),params![team,node,generation],|r|r.get::<_,String>(0)).optional().map_err(storage)
}
impl Store {
    /// Integration branch creation has a permanent origin, separate from the
    /// default for newly selected member launches and hand-outs.
    pub(crate) fn creation_base(&self, team: &str) -> Result<String> {
        self.active()?;
        self.db.as_ref().ok_or_else(|| storage("Missing team store"))?
            .query_row("SELECT COALESCE(o.base_commit,t.base_commit) FROM teams t LEFT JOIN base_origins o ON o.team=t.id WHERE t.id=?1",[team],|r|r.get(0))
            .map_err(storage)
    }
    pub(crate) fn effect_base(&self, team: &str, effect: &str) -> Result<String> {
        self.active()?;
        self.db
            .as_ref()
            .ok_or_else(|| storage("Missing team store"))?
            .query_row(
                &format!(
                    "SELECT {VALUE} FROM effects e {JOINS} WHERE e.team=?1 AND e.request_id=?2"
                ),
                params![team, effect],
                |r| r.get(0),
            )
            .map_err(storage)
    }
    pub(crate) fn attempt_base(
        &self,
        team: &str,
        node: u32,
        generation: u32,
    ) -> Result<Option<String>> {
        self.active()?;
        self.db
            .as_ref()
            .map_or(Ok(None), |db| attempt(db, team, node, generation))
    }
}
