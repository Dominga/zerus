//! Task 115: a Kimi team member's narrow allow rules for the read-only hgs
//! commands it needs. Kimi asks for every Bash call outside its default
//! allowlist, so a member read of `hgs team status --json` or `hgs guide team`
//! reached the lead as a permission. Zerus writes these rules into the Kimi
//! config the member's launch reads (`<home>/config.toml`, `[[permission.rules]]`),
//! only adding the read-only patterns: every command that changes anything
//! keeps asking, and no broad bypass is ever written. The rest of the member's
//! config is left as it is; a config that is not valid TOML is refused, never
//! rewritten.
use anyhow::{anyhow, ensure, Result};
use std::path::Path;

/// The read-only hgs commands a team member may run without an approval: the
/// team it belongs to, its own hand-out, the guide and an inspection of a
/// session. Nothing here changes a team, a task, a file or a session.
pub(crate) const READ_ONLY: [&str; 6] = [
    "hgs team status",
    "hgs team current",
    "hgs team inbox",
    "hgs team profile",
    "hgs guide",
    "hgs inspect",
];

/// The largest Kimi config this writer accepts (the file holds a provider
/// table and models, never anything near this).
const MAX_CONFIG: u64 = 1024 * 1024;

/// Kimi's rule pattern for one command: its Bash matcher is a glob over the
/// whole command, so `hgs team status*` takes the bare and the `--json` forms.
/// A command whose first word is not the exact read-only command is absent.
fn patterns(command: &str) -> [String; 2] {
    [format!("Bash({command})"), format!("Bash({command} *)")]
}

/// Every pattern these rules allow; a test and the caller use it to keep the
/// set narrow.
pub(crate) fn read_only_patterns() -> Vec<String> {
    READ_ONLY
        .iter()
        .flat_map(|command| patterns(command))
        .collect()
}

/// Adds the read-only rules to the Kimi config at `home` when they are absent.
/// Only `[[permission.rules]]` entries are appended: the existing tables, the
/// existing rules (a deny included) and any other key stay as they were. An
/// already handled config is not written again. Returns whether it changed.
pub(crate) fn allow_read_only_hgs(home: &Path) -> Result<bool> {
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
    let mut document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|_| anyhow!("the Kimi config is not valid TOML"))?;
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
    let mut changed = false;
    for pattern in read_only_patterns() {
        if rules.iter().any(|rule| {
            rule.get("pattern").and_then(|pattern| pattern.as_str()) == Some(pattern.as_str())
        }) {
            continue;
        }
        let mut rule = toml_edit::Table::new();
        rule.insert("decision", toml_edit::value("allow"));
        rule.insert("pattern", toml_edit::value(pattern));
        rules.push(rule);
        changed = true;
    }
    if !changed {
        return Ok(false);
    }
    write(&path, &document.to_string())?;
    Ok(true)
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

    #[test]
    fn the_read_only_rules_join_a_recorded_kimi_config_and_nothing_else_changes() {
        let home = scratch("join");
        std::fs::write(home.join("config.toml"), RECORDED).unwrap();
        assert!(allow_read_only_hgs(&home).unwrap());
        let document = parsed(&home);
        // The provider, model, thinking, services and guard are untouched.
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
        // The recorded deny stays first; every read-only pattern follows, once.
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
        assert!(rules
            .iter()
            .all(|rule| rule["decision"].as_str() == Some("allow")
                || rule["pattern"].as_str() == Some("Bash(rm -rf*)")));
        // Every allow names a read-only command and a *-glob, never a bare *.
        for rule in rules
            .iter()
            .filter(|rule| rule["decision"].as_str() == Some("allow"))
        {
            let pattern = rule["pattern"].as_str().unwrap();
            assert_ne!(pattern, "Bash(*)");
            let command = pattern
                .strip_prefix("Bash(")
                .and_then(|p| p.strip_suffix(')'))
                .unwrap();
            let command = command.strip_suffix(" *").unwrap_or(command);
            assert!(READ_ONLY.contains(&command), "{pattern}");
        }
        // A second write changes nothing, and the file is still valid TOML.
        let before = std::fs::read(home.join("config.toml")).unwrap();
        assert!(!allow_read_only_hgs(&home).unwrap());
        assert_eq!(std::fs::read(home.join("config.toml")).unwrap(), before);
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

    #[test]
    fn an_identical_rule_is_not_duplicated_and_an_empty_home_gets_a_valid_config() {
        let home = scratch("empty");
        assert!(allow_read_only_hgs(&home).unwrap());
        let document = parsed(&home);
        assert_eq!(
            document["permission"]["rules"]
                .as_array_of_tables()
                .unwrap()
                .len(),
            read_only_patterns().len()
        );
        // Adding one of the patterns by hand leaves it as the member wrote it;
        // the writer adds only the missing ones and never removes a copy.
        let text = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let mut hand: toml_edit::DocumentMut = text.parse().unwrap();
        {
            let rules = hand["permission"]["rules"]
                .as_array_of_tables_mut()
                .unwrap();
            let mut rule = toml_edit::Table::new();
            rule.insert("decision", toml_edit::value("allow"));
            rule.insert("pattern", toml_edit::value("Bash(hgs team status)"));
            rules.push(rule);
        }
        std::fs::write(home.join("config.toml"), hand.to_string()).unwrap();
        assert!(
            !allow_read_only_hgs(&home).unwrap(),
            "every pattern already exists"
        );
        let document = parsed(&home);
        let count = document["permission"]["rules"]
            .as_array_of_tables()
            .unwrap()
            .iter()
            .filter(|rule| rule["pattern"].as_str() == Some("Bash(hgs team status)"))
            .count();
        assert_eq!(
            count, 2,
            "the hand-written copy is kept, the writer adds only the missing one"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_config_that_is_not_valid_toml_is_refused_and_left_alone() {
        let home = scratch("invalid");
        std::fs::write(home.join("config.toml"), "default_model = [unterminated\n").unwrap();
        let before = std::fs::read(home.join("config.toml")).unwrap();
        assert!(allow_read_only_hgs(&home).is_err());
        assert_eq!(std::fs::read(home.join("config.toml")).unwrap(), before);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_permission_section_holding_something_else_is_refused_not_replaced() {
        let home = scratch("permission-table");
        std::fs::write(
            home.join("config.toml"),
            "[permission]\nrules = \"wrong\"\n",
        )
        .unwrap();
        let before = std::fs::read(home.join("config.toml")).unwrap();
        assert!(allow_read_only_hgs(&home).is_err());
        assert_eq!(std::fs::read(home.join("config.toml")).unwrap(), before);
        let _ = std::fs::remove_dir_all(&home);
    }
}
