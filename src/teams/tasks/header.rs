//! Narrow indexed team reads. Ordinary task guards never decode a bundle.
use super::*;
use zerus_team_plan::model::{Settings, TeamStatus};

pub(super) struct Header {
    pub id: String,
    pub project: String,
    pub name: String,
    pub home: String,
    pub owner: String,
    pub lead: String,
    pub status: TeamStatus,
    pub settings: Settings,
    pub revision: u64,
}
impl Header {
    pub fn live(&self) -> bool {
        !matches!(
            self.status,
            TeamStatus::Stopped | TeamStatus::Archived | TeamStatus::Finished
        )
    }
    pub fn queue(&self) -> bool {
        matches!(self.status, TeamStatus::Running | TeamStatus::Paused) && self.settings.queue
    }
}
pub(super) fn read(store: &Store, id: &str) -> Result<Option<Header>> {
    store.db.as_ref().map_or(Ok(None),|db|db.query_row("SELECT id,project,name,home,owner,lead,status,settings,revision FROM teams WHERE id=?1",[id],|r|Ok(Header{
        id:r.get(0)?,project:r.get(1)?,name:r.get(2)?,home:r.get(3)?,owner:r.get(4)?,lead:r.get(5)?,status:super::super::store::sql_json(r,6)?,settings:super::super::store::sql_json(r,7)?,revision:r.get(8)?,
    })).optional().map_err(storage))
}
