//! Zerus account policy, never a write to Kimi's own tui.toml.
use super::*;

pub(super) fn set(config: &Config, args: &[String], dry: bool) -> Result<Value> {
    let id = args.get(1).map(String::as_str).unwrap_or("");
    let enabled = match arg(args, "--enabled") {
        Some("true") => true,
        Some("false") => false,
        _ => return Err(Error::new(1, "Choose --enabled true or false.")),
    };
    let _guard = locked(config)?;
    let bytes = read(config)?;
    check_revision(&bytes, args)?;
    let mut catalog = decode(&bytes)?;
    let (profile, _, _) = lookup(config, id)?;
    if profile.provider != "kimi" || catalog.hidden_profiles.contains(id) {
        return Err(Error::new(1, "Choose an available Kimi account."));
    }
    catalog.kimi_compact_on_expiry.insert(id.into(), enabled);
    if !dry { save(config, &catalog)?; }
    list(config)
}

pub(super) fn enabled(config: &Config, id: &str, home: &Path) -> Result<bool> {
    let catalog = decode(&read(config)?)?;
    let (profile, expected_home, _) = lookup(config, id)?;
    if profile.provider != "kimi" || expected_home != home || catalog.hidden_profiles.contains(id) {
        return Err(Error::new(1, "The Kimi account binding changed."));
    }
    Ok(catalog.kimi_compact_on_expiry.get(id).copied().unwrap_or(true))
}
