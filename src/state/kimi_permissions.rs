//! Task 115: the owner's explicit opt-in for a Kimi account's read-only hgs
//! rules. A team member's launch never edits a Kimi config by itself: Kimi has
//! no per-launch rule override (only a whole-home `KIMI_CODE_HOME`, which
//! would move its OAuth credentials), so the rules are written only through
//! `hgs account kimi-read-only`, after the owner confirms the exact patterns.
//! The writer adds `[[permission.rules]]` allow entries for the read-only
//! team commands and nothing else: every command that changes anything keeps
//! asking, and no broad bypass is ever written. The rest of the config is left
//! as it is; a config that is not valid TOML is refused, never rewritten.
use anyhow::{anyhow, ensure, Result};
use std::path::{Path, PathBuf};

/// The read-only hgs commands a team member may run without an approval: the
/// team it belongs to, its own hand-out and the guide. Nothing here changes a
/// team, a task, a file or a session. Task 90: `hgs inspect` is not here: it
/// takes a session name, which only a `*` pattern can allow, and a `*` pattern
/// also lets `hgs inspect x; other` through a matcher that does not split
/// compound commands (Kimi's 2.5.2 matcher could not be checked here).
pub(crate) const READ_ONLY: [&str; 6] = [
    "hgs team status",
    "hgs team current",
    "hgs team inbox",
    "hgs team profile",
    "hgs guide",
    "hgs guide team",
];

/// The largest Kimi config this writer accepts (the file holds a provider
/// table and models, never anything near this).
const MAX_CONFIG: u64 = 1024 * 1024;

/// Kimi's rule patterns for one command: exactly the bare command and its
/// `--json` form, never a `*` glob, so no command with anything appended (a
/// `;`, `&&` or `|` and another command) can match, whether or not Kimi's
/// matcher splits compound commands.
fn patterns(command: &str) -> [String; 2] {
    [format!("Bash({command})"), format!("Bash({command} --json)")]
}

/// Every pattern the opt-in may add; the confirmation names exactly these.
pub(crate) fn read_only_patterns() -> Vec<String> {
    READ_ONLY
        .iter()
        .flat_map(|command| patterns(command))
        .collect()
}

/// What the owner is shown before anything is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Preview {
    pub home: PathBuf,
    pub config: PathBuf,
    /// Every rule this opt-in manages, in confirmation order.
    pub patterns: Vec<String>,
    pub present: Vec<String>,
    pub missing: Vec<String>,
}

/// What the owner-approved write did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Write {
    /// The owner did not confirm: nothing was read or written.
    Refused,
    /// Every rule was already there.
    Unchanged,
    Written,
}

/// The rules still missing from the config at `home`, without writing.
pub(crate) fn preview(home: &Path) -> Result<Preview> {
    let (_, document) = parse(home)?;
    let existing = patterns_in(&document);
    let patterns = read_only_patterns();
    let present: Vec<String> = patterns
        .iter()
        .filter(|pattern| existing.iter().any(|have| have == *pattern))
        .cloned()
        .collect();
    let missing: Vec<String> = patterns
        .iter()
        .filter(|pattern| !existing.iter().any(|have| have == *pattern))
        .cloned()
        .collect();
    Ok(Preview {
        home: home.to_path_buf(),
        config: home.join("config.toml"),
        patterns,
        present,
        missing,
    })
}

/// Adds the read-only rules to the Kimi config at `home` when the owner
/// confirmed. Without `confirm` nothing is read or written: the caller shows
/// the preview first. Only `[[permission.rules]]` entries are appended; the
/// existing tables, the existing rules (a deny included) and any other key stay
/// as they were. Returns what happened.
pub(crate) fn apply(home: &Path, confirm: bool) -> Result<Write> {
    if !confirm {
        return Ok(Write::Refused);
    }
    let (path, mut document) = parse(home)?;
    let existing = patterns_in(&document);
    let missing: Vec<String> = read_only_patterns()
        .into_iter()
        .filter(|pattern| !existing.iter().any(|have| have == pattern))
        .collect();
    if missing.is_empty() {
        return Ok(Write::Unchanged);
    }
    if !document.contains_key("permission") {
        document["permission"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let permission = document["permission"]
        .as_table_mut()
        .ok_or_else(|| anyhow!("the Kimi permission section is not a table"))?;
    if !permission.contains_key("rules") {
        permission["rules"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let rules = permission["rules"]
        .as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("the Kimi permission rules are not a list of tables"))?;
    for pattern in missing {
        let mut rule = toml_edit::Table::new();
        rule.insert("decision", toml_edit::value("allow"));
        rule.insert("pattern", toml_edit::value(pattern));
        rules.push(rule);
    }
    write(&path, &document.to_string())?;
    Ok(Write::Written)
}

/// The owner's explicit opt-out: removes only the rules this opt-in manages,
/// keeping every other rule and table. Returns how many rules were removed.
pub(crate) fn remove(home: &Path) -> Result<usize> {
    let (path, mut document) = parse(home)?;
    let managed = read_only_patterns();
    let Some(permission) = document
        .get_mut("permission")
        .and_then(toml_edit::Item::as_table_mut)
    else {
        return Ok(0);
    };
    let Some(rules) = permission
        .get_mut("rules")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
    else {
        return Ok(0);
    };
    let mut removed = 0;
    let mut kept = toml_edit::ArrayOfTables::new();
    for rule in rules.iter() {
        let managed_here = rule
            .get("pattern")
            .and_then(toml_edit::Item::as_str)
            .is_some_and(|pattern| managed.iter().any(|known| known == pattern));
        if managed_here {
            removed += 1;
        } else {
            kept.push(rule.clone());
        }
    }
    if removed == 0 {
        return Ok(0);
    }
    *rules = kept;
    write(&path, &document.to_string())?;
    Ok(removed)
}

/// The config path at `home` and its parsed document; a missing config is an
/// empty one, and an unsupported one is refused rather than replaced.
fn parse(home: &Path) -> Result<(PathBuf, toml_edit::DocumentMut)> {
    let path = home.join("config.toml");
    let text = match std::fs::read(&path) {
        Ok(bytes) => {
            ensure!(
                bytes.len() as u64 <= MAX_CONFIG,
                "the Kimi config is too large to edit"
            );
            String::from_utf8(bytes).map_err(|_| anyhow!("the Kimi config is not UTF-8"))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|_| anyhow!("the Kimi config is not valid TOML"))?;
    Ok((path, document))
}

/// The patterns the config already allows; a deny with the same pattern does
/// not count, so an allow is still added when Kimi would ask.
fn patterns_in(document: &toml_edit::DocumentMut) -> Vec<String> {
    document
        .get("permission")
        .and_then(toml_edit::Item::as_table)
        .and_then(|permission| permission.get("rules"))
        .and_then(toml_edit::Item::as_array_of_tables)
        .map(|rules| {
            rules
                .iter()
                .filter(|rule| {
                    rule.get("decision").and_then(toml_edit::Item::as_str) == Some("allow")
                })
                .filter_map(|rule| rule.get("pattern").and_then(toml_edit::Item::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Replaces the config atomically: a sibling file first, then a rename, so a
/// member launch never reads half a config. The new file is owner-only, as the
/// provider keys it holds make it private.
fn write(path: &Path, text: &str) -> Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| anyhow!("the Kimi config has no home"))?;
    std::fs::create_dir_all(folder)?;
    let temporary = folder.join(format!(".config.toml.zerus-{}.tmp", std::process::id()));
    std::fs::write(&temporary, text)?;
    let _ = crate::platform::owner_only_file(&temporary);
    std::fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recorded Kimi Code config (`~/.kimi-code/config.toml`), trimmed: the
    /// provider and model tables a real one carries, thinking, services, and a
    /// permission section with a deny rule and a guard the writer must keep.
    const RECORDED: &str = "default_model = \"kimi-code/kimi-for-coding\"\n\
\n\
[providers.\"managed:kimi-code\"]\n\
type = \"kimi\"\n\
api_key = \"\"\n\
base_url = \"https://api.kimi.com/coding/v1\"\n\
\n\
[providers.\"managed:kimi-code\".oauth]\n\
storage = \"file\"\n\
key = \"oauth/kimi-code\"\n\
\n\
[models.\"kimi-code/kimi-for-coding\"]\n\
provider = \"managed:kimi-code\"\n\
model = \"kimi-for-coding\"\n\
max_context_size = 262144\n\
capabilities = [ \"thinking\", \"tool_use\" ]\n\
display_name = \"kimi-for-coding\"\n\
\n\
[thinking]\n\
enabled = true\n\
\n\
[services.moonshot_search]\n\
base_url = \"https://api.kimi.com/coding/v1/search\"\n\
\n\
[permission]\n\
dangerous_command_guard = true\n\
\n\
[[permission.rules]]\n\
decision = \"deny\"\n\
pattern = \"Bash(rm -rf*)\"\n";

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("zerus-kimi-rules-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn parsed(home: &Path) -> toml_edit::DocumentMut {
        std::fs::read_to_string(home.join("config.toml"))
            .unwrap()
            .parse()
            .unwrap()
    }

    /// Task 115: the launch never writes; only the owner's confirmed opt-in
    /// does. Without `confirm` the preview is read-only.
    #[test]
    fn the_rules_are_written_only_when_the_owner_confirms() {
        let home = scratch("confirm");
        std::fs::write(home.join("config.toml"), RECORDED).unwrap();
        let before = std::fs::read(home.join("config.toml")).unwrap();
        let shown = preview(&home).unwrap();
        assert_eq!(shown.patterns.len(), read_only_patterns().len());
        assert_eq!(shown.present.len(), 0);
        assert_eq!(
            shown.missing, shown.patterns,
            "the confirmation names every rule"
        );
        assert_eq!(apply(&home, false).unwrap(), Write::Refused);
        assert_eq!(
            std::fs::read(home.join("config.toml")).unwrap(),
            before,
            "nothing was written"
        );
        assert_eq!(apply(&home, true).unwrap(), Write::Written);
        let document = parsed(&home);
        // The provider, model, thinking, services, guard and deny are untouched.
        assert_eq!(
            document["default_model"].as_str(),
            Some("kimi-code/kimi-for-coding")
        );
        assert_eq!(
            document["providers"]["managed:kimi-code"]["base_url"].as_str(),
            Some("https://api.kimi.com/coding/v1")
        );
        assert_eq!(
            document["models"]["kimi-code/kimi-for-coding"]["max_context_size"].as_integer(),
            Some(262144)
        );
        assert_eq!(document["thinking"]["enabled"].as_bool(), Some(true));
        assert_eq!(
            document["services"]["moonshot_search"]["base_url"].as_str(),
            Some("https://api.kimi.com/coding/v1/search")
        );
        assert_eq!(
            document["permission"]["dangerous_command_guard"].as_bool(),
            Some(true)
        );
        let rules = document["permission"]["rules"]
            .as_array_of_tables()
            .unwrap();
        assert_eq!(rules.get(0).unwrap()["decision"].as_str(), Some("deny"));
        assert_eq!(
            rules.get(0).unwrap()["pattern"].as_str(),
            Some("Bash(rm -rf*)")
        );
        let mut seen: Vec<String> = rules
            .iter()
            .filter_map(|rule| rule.get("pattern").and_then(|pattern| pattern.as_str()))
            .map(str::to_owned)
            .collect();
        let mut expected = read_only_patterns();
        expected.push("Bash(rm -rf*)".into());
        expected.sort();
        seen.sort();
        assert_eq!(seen, expected);
        // Every allow names one read-only command, bare or with --json, and no
        // glob at all.
        for rule in rules
            .iter()
            .filter(|rule| rule["decision"].as_str() == Some("allow"))
        {
            let pattern = rule["pattern"].as_str().unwrap();
            assert!(!pattern.contains('*'), "{pattern}");
            let command = pattern
                .strip_prefix("Bash(")
                .and_then(|p| p.strip_suffix(')'))
                .unwrap();
            let command = command.strip_suffix(" --json").unwrap_or(command);
            assert!(READ_ONLY.contains(&command), "{pattern}");
        }
        // Confirmed again: nothing more to write.
        assert_eq!(preview(&home).unwrap().missing, Vec::<String>::new());
        assert_eq!(apply(&home, true).unwrap(), Write::Unchanged);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(home.join("config.toml"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    /// The opt-out removes only the rules this opt-in manages, and leaves a
    /// hand-written rule and the deny alone.
    #[test]
    fn the_opt_out_removes_only_the_rules_it_wrote() {
        let home = scratch("remove");
        std::fs::write(home.join("config.toml"), RECORDED).unwrap();
        assert_eq!(apply(&home, true).unwrap(), Write::Written);
        // A hand-written allow with another pattern is not this opt-in's.
        let text = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let mut hand: toml_edit::DocumentMut = text.parse().unwrap();
        {
            let rules = hand["permission"]["rules"]
                .as_array_of_tables_mut()
                .unwrap();
            let mut rule = toml_edit::Table::new();
            rule.insert("decision", toml_edit::value("allow"));
            rule.insert("pattern", toml_edit::value("Bash(git status*)"));
            rules.push(rule);
        }
        std::fs::write(home.join("config.toml"), hand.to_string()).unwrap();
        assert_eq!(remove(&home).unwrap(), read_only_patterns().len());
        let document = parsed(&home);
        let patterns: Vec<&str> = document["permission"]["rules"]
            .as_array_of_tables()
            .unwrap()
            .iter()
            .filter_map(|rule| rule.get("pattern").and_then(toml_edit::Item::as_str))
            .collect();
        assert_eq!(patterns, ["Bash(rm -rf*)", "Bash(git status*)"]);
        assert_eq!(remove(&home).unwrap(), 0, "nothing managed is left");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_config_that_is_not_valid_toml_is_refused_and_left_alone() {
        let home = scratch("invalid");
        std::fs::write(home.join("config.toml"), "default_model = [unterminated\n").unwrap();
        let before = std::fs::read(home.join("config.toml")).unwrap();
        assert!(preview(&home).is_err());
        assert!(apply(&home, true).is_err());
        assert_eq!(std::fs::read(home.join("config.toml")).unwrap(), before);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_empty_home_gets_a_valid_config_only_after_confirmation() {
        let home = scratch("empty");
        assert_eq!(
            preview(&home).unwrap().missing.len(),
            read_only_patterns().len()
        );
        assert_eq!(apply(&home, false).unwrap(), Write::Refused);
        assert!(!home.join("config.toml").exists());
        assert_eq!(apply(&home, true).unwrap(), Write::Written);
        let document = parsed(&home);
        assert_eq!(
            document["permission"]["rules"]
                .as_array_of_tables()
                .unwrap()
                .len(),
            read_only_patterns().len()
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
