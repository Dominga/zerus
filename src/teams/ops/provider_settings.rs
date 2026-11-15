use super::*;

pub(super) fn select(ctx: &mut Context<'_>, config: Option<&crate::config::Config>) -> Result<()> {
    let Operation::SetValidationConfig { selection } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    selection.check().map_err(|s| invalid(&s))?;
    let config = config
        .ok_or_else(|| invalid("Provider selection requires the native project configuration."))?;
    if crate::platform::canonical(&config.state).map_err(storage)? != ctx.state {
        return Err(invalid(
            "Provider selection belongs to another state directory.",
        ));
    }
    // Re-resolve the native-selected project and every bounded source while
    // holding this team's writer transaction. The renderer never names a path.
    let fresh = crate::teams::providers::inspect(
        config,
        ctx.db,
        ctx.team,
        Some(selection.override_config.clone()),
    )?;
    if !fresh.valid || &fresh.selection() != selection {
        return Err(Problem::new(
            "config_changed",
            "The checkout or provider configuration changed. Check it again before saving.",
        ));
    }
    crate::teams::providers::write(ctx.db, &ctx.team.id, selection)
}
