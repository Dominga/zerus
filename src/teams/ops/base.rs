//! Person-only, read-only Git validation inside the operation transaction.
use super::*;

pub(super) fn advance(
    ctx: &Context<'_>,
    repository: &dyn Fn(&Team) -> Result<crate::teams::integrate::Repo>,
) -> Result<()> {
    let Operation::AdvanceBase { commit } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    if ctx.team.settings.integration != Integration::Branch {
        return Err(invalid(
            "Base advancement requires a team with branch integration.",
        ));
    }
    let repo = repository(ctx.team)?;
    let main_ref =
        crate::teams::integrate::validate_base_advance(&repo, &ctx.team.base_commit, commit)?;
    ctx.db.execute("INSERT OR IGNORE INTO base_origins(team,base_commit,legacy_ceiling) SELECT ?1,?2,COALESCE(MAX(seq),0) FROM effects WHERE team=?1",params![ctx.team.id,ctx.team.base_commit]).map_err(storage)?;
    crate::teams::store::bases::protect_writers(ctx.db)?;
    ctx.db
        .execute(
            "UPDATE teams SET base_commit=?2 WHERE id=?1",
            params![ctx.team.id, commit],
        )
        .map_err(storage)?;
    ctx.db.execute("INSERT INTO base_advances(team,revision,previous,commit_id,main_ref) VALUES(?1,?2,?3,?4,?5)",
        params![ctx.team.id,ctx.team.revision.checked_add(1).filter(|r| *r <= i64::MAX as u64).ok_or_else(||over("Revision is exhausted."))?,ctx.team.base_commit,commit,main_ref]).map_err(storage)?;
    Ok(())
}
