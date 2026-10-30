//! Local document references in Activity, ported from SessionFileReference.h.
//! Relative paths resolve on the session's machine, never against the GUI cwd.

use crate::paths::{self, Style};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub path: String,
    pub location: String,
}

impl Target {
    pub fn valid(&self) -> bool {
        !self.path.is_empty()
    }
}

fn decoded(text: &str) -> Option<String> {
    if !text.bytes().all(|byte| byte.is_ascii()) {
        return Some(text.to_owned());
    }
    let mut out = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'%' {
            out.push(bytes[at]);
            at += 1;
            continue;
        }
        if at + 2 >= bytes.len() {
            return None;
        }
        let hex = |byte: u8| -> Option<u8> {
            match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            }
        };
        out.push(hex(bytes[at + 1])? << 4 | hex(bytes[at + 2])?);
        at += 3;
    }
    String::from_utf8(out).ok()
}

fn valid_text(text: &str) -> bool {
    text.chars().all(|c| !c.is_control())
}

fn line_suffix(path: &str) -> Option<(usize, String)> {
    let last = path.rfind(':')?;
    let column = path.get(last + 1..)?;
    if column.is_empty() || !column.bytes().all(|b| b.is_ascii_digit()) || column.starts_with('0') {
        return None;
    }
    if let Some(previous) = path[..last].rfind(':') {
        let line = path.get(previous + 1..last)?;
        if !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()) && !line.starts_with('0') {
            return Some((previous, path[previous..].to_owned()));
        }
    }
    Some((last, path[last..].to_owned()))
}

pub fn parse(href: &str, style: Style) -> Target {
    let windows = style == Style::Windows;
    let windows_drive = windows && paths::absolute(href, style) && !href.starts_with(['/', '\\']);
    let source = if windows_drive {
        format!("file:///{}", href.replace('\\', "%5C"))
    } else {
        href.to_owned()
    };
    if source.contains('?') {
        return Target::default();
    }
    let (before_fragment, fragment) = source
        .split_once('#')
        .map_or((source.as_str(), ""), |parts| parts);
    let scheme_end = before_fragment
        .find(':')
        .filter(|at| !before_fragment[..*at].contains('/'));
    let scheme = scheme_end.map_or("", |at| &before_fragment[..at]);
    if !scheme.is_empty() && scheme != "file" {
        return Target::default();
    }
    let remainder = scheme_end.map_or(before_fragment, |at| &before_fragment[at + 1..]);
    let authority = remainder.strip_prefix("//");
    let path_text = if let Some(authority) = authority {
        let host_end = authority.find('/').unwrap_or(authority.len());
        let host = &authority[..host_end];
        if !host.is_empty() || host.contains('@') {
            return Target::default();
        }
        &authority[host_end..]
    } else {
        remainder
    };
    let Some(mut path) = decoded(path_text) else {
        return Target::default();
    };
    if windows && path.starts_with('/') && paths::absolute(&path[1..], style) {
        path.remove(0);
    }
    if path.is_empty() || path.starts_with("//") || path.len() > 8192 || !valid_text(&path) {
        return Target::default();
    }
    if windows
        && path.len() > 1
        && path.starts_with(['/', '\\'])
        && path[1..].starts_with(['/', '\\'])
    {
        return Target::default();
    }
    let Some(location) = decoded(fragment) else {
        return Target::default();
    };
    if location.len() > 2048 || !valid_text(&location) {
        return Target::default();
    }
    let mut location = if location.is_empty() {
        String::new()
    } else {
        format!("#{location}")
    };
    if location.is_empty() {
        if let Some((at, suffix)) = line_suffix(&path) {
            location = suffix;
            path.truncate(at);
        }
    }
    Target { path, location }
}

pub fn resolve(target: &Target, cwd: &str, local_home: Option<&str>, style: Style) -> String {
    if !target.valid() {
        return String::new();
    }
    if paths::absolute(&target.path, style) {
        return paths::clean(&target.path, style);
    }
    if let Some(rest) = target.path.strip_prefix("~/") {
        return local_home.map_or_else(String::new, |home| {
            paths::clean(&paths::join(home, rest), style)
        });
    }
    if !paths::absolute(cwd, style) {
        return String::new();
    }
    paths::clean(&paths::join(cwd, &target.path), style)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_file_urls_keep_their_location() {
        for href in [
            "/work/a report.md#part",
            "file:///work/a%20report.md:12:4",
            "docs/notes.md:8",
        ] {
            let target = parse(href, Style::Posix);
            assert!(target.valid(), "{href}");
            assert!(!target.location.is_empty(), "{href}");
        }
        assert_eq!(
            parse("file:///work/a%20report.md:12:4", Style::Posix),
            Target {
                path: "/work/a report.md".into(),
                location: ":12:4".into()
            }
        );
        assert_eq!(
            parse("docs/notes.md:8", Style::Posix),
            Target {
                path: "docs/notes.md".into(),
                location: ":8".into()
            }
        );
        assert_eq!(
            parse("/work/notes.md:12", Style::Posix),
            Target {
                path: "/work/notes.md".into(),
                location: ":12".into()
            }
        );
    }

    #[test]
    fn windows_paths_work_on_every_host() {
        assert_eq!(
            parse(r"C:\work\a.cpp:12", Style::Windows),
            Target {
                path: r"C:\work\a.cpp".into(),
                location: ":12".into()
            }
        );
        assert_eq!(
            parse("file:///C:/work/a.cpp", Style::Windows).path,
            "C:/work/a.cpp"
        );
        assert!(!parse(r"\\server\share\a.cpp", Style::Windows).valid());
        assert!(!parse(r"C:\work\a.cpp", Style::Posix).valid());
    }

    #[test]
    fn unsafe_or_foreign_references_are_rejected() {
        for href in [
            "https://example.com/a",
            "file://host/a",
            "file:///tmp/a?download=1",
            "file://user@/a",
            "//server/share/a",
            "file:///tmp/%ZZ",
        ] {
            assert!(!parse(href, Style::Posix).valid(), "{href}");
        }
    }

    #[test]
    fn resolution_uses_the_sessions_cwd_and_only_local_home() {
        let target = parse("docs/notes.md:8", Style::Posix);
        assert_eq!(
            resolve(&target, "/work/session", None, Style::Posix),
            "/work/session/docs/notes.md"
        );
        let home = parse("~/notes.md", Style::Posix);
        assert_eq!(
            resolve(&home, "/work/session", Some("/home/user"), Style::Posix),
            "/home/user/notes.md"
        );
        assert_eq!(resolve(&home, "/work/session", None, Style::Posix), "");
        let relative = parse("../shared/notes.md", Style::Posix);
        assert_eq!(
            resolve(&relative, "/work/session", None, Style::Posix),
            "/work/shared/notes.md"
        );
    }
}
