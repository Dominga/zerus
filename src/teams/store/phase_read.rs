//! The existing read transaction selects one current phase source. An open
//! rework retains BASE; this grants no selector for arbitrary older versions.
use super::*;

pub(super) struct Source {
    pub episode: Option<rework::ReworkRow>,
    pub publication: Option<publication::Publication>,
}

pub(super) fn source(db: &Connection, team: &str, phase: u32) -> Result<Source> {
    let episode = rework::latest(db, team, phase)?;
    let held = episode.as_ref().filter(|r| r.state != "published");
    let publication = if let Some(r) = held {
        Some(
            publication::by_id(db, team, &r.base_publication)?
                .ok_or_else(|| storage("The rework base publication is missing."))?,
        )
    } else {
        publication::publication(db, team, phase)?
    };
    Ok(Source {
        episode,
        publication,
    })
}
