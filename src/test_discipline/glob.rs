//! Path globs of the test configuration: `**` spans folders, `*` and `?` stay
//! inside one name, and a pattern without a `/` matches a name in any folder.

pub fn matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim_start_matches("./");
    if !pattern.contains('/') {
        return path.rsplit('/').next().is_some_and(|name| segment(pattern.as_bytes(), name.as_bytes()));
    }
    let parts: Vec<&str> = pattern.split('/').collect();
    let names: Vec<&str> = path.split('/').collect();
    segments(&parts, &names)
}

pub fn any(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|pattern| matches(pattern, path))
}

fn segments(parts: &[&str], names: &[&str]) -> bool {
    match parts.split_first() {
        None => names.is_empty(),
        Some((&"**", rest)) => (0..=names.len()).any(|skip| segments(rest, &names[skip..])),
        Some((part, rest)) => {
            names.split_first().is_some_and(|(name, others)| segment(part.as_bytes(), name.as_bytes()) && segments(rest, others))
        }
    }
}

fn segment(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|skip| segment(rest, &name[skip..])),
        Some((b'?', rest)) => !name.is_empty() && segment(rest, &name[1..]),
        Some((byte, rest)) => name.first() == Some(byte) && segment(rest, &name[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn globs_span_folders_only_with_two_stars() {
        assert!(matches("src/**", "src/state/teams/ops.rs"));
        assert!(matches("src/**/*.rs", "src/main.rs"));
        assert!(matches("tray/tests/test_*.cpp", "tray/tests/test_teams.cpp"));
        assert!(!matches("tray/tests/test_*.cpp", "tray/tests/support/test_x.cpp"));
        assert!(!matches("src/*.rs", "src/state/mod.rs"));
        assert!(matches("*.md", "docs/plans/a.md"));
        assert!(matches("Cargo.lock", "Cargo.lock"));
        assert!(matches("tests/test_?.py", "tests/test_a.py"));
        assert!(!matches("src/**", "srcs/x.rs"));
    }
}
