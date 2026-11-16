//! Cumulative native admission storage. A floor never reconstructs missing
//! current evidence, invents a rework fact or downgrades an existing store.
use super::*;

pub(super) fn validate(db: &Connection) -> Result<()> {
    native_person::validate(db)?;
    publication::validate(db)?;
    rework::validate(db)?;
    person_actions::validate(db)
}

/// Legacy four may predate publication entirely. Once publication or rework
/// evidence exists, validate it before any additive extension could repair it.
pub(super) fn legacy(db: &Connection, version: u32) -> Result<()> {
    if version >= versions::CORE_PUBLICATION || publication::has_evidence(db)? {
        publication::validate(db)?;
    }
    rework::validate(db)
}

pub(super) fn migrate(db: &Connection) -> Result<()> {
    if db.is_autocommit() {
        return Err(storage(
            "Native admission migration requires the writer transaction.",
        ));
    }
    native_person::migrate(db)?;
    rework::native_floor(db)?;
    person_actions::migrate(db)?;
    validate(db)
}
