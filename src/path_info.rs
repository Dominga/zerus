//! `hgs [@host] path-info --path <path>`: whether a path exists on this
//! computer, its kind and, for a file, its size. One metadata call; no
//! content is read and nothing is changed. The reference-file dialog asks a
//! session's computer before it inserts a remote path (zerus-5wmh.13.18).
//!
//! The reply is one line of JSON, versioned and closed:
//! `{"schema": 1, "exists": true|false|null, "kind": "file"|"directory"|"other"|null, "size": N|null}`.
//! `exists` is null when the path cannot be inspected (for example a
//! folder this account may not read); `size` is a file's length only.

use crate::cli::{Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The reply's version: a client accepts only the schemas it knows.
pub const SCHEMA: u64 = 1;
/// The longest path accepted, as the dialog's.
const MAX_PATH: usize = 32 << 10;
pub const USAGE: &str = "usage: hgs [@host] path-info --path <path>";

/// `~` is the home folder, `~/rest` a path under it (the remaining
/// separators stay under home, as Qt's `homePath() + path.mid(1)`); any
/// other path must be absolute. The rest is appended as text: `join` would
/// let a drive or root in it replace home (zerus-fqsli).
fn expanded(path: &str, home: Option<&Path>) -> std::result::Result<PathBuf, &'static str> {
    if path.is_empty() || path.len() > MAX_PATH || path.chars().any(char::is_control) {
        return Err("enter a path without line breaks or control characters");
    }
    let target = if path == "~" {
        home.ok_or("this computer has no home folder")?
            .to_path_buf()
    } else if let Some(rest) = path.strip_prefix("~/") {
        let home = home.ok_or("this computer has no home folder")?;
        let rest = rest.trim_start_matches(std::path::is_separator);
        if rest.is_empty() {
            home.to_path_buf()
        } else {
            let mut joined = home.as_os_str().to_owned();
            if !joined
                .as_encoded_bytes()
                .last()
                .is_some_and(|byte| std::path::is_separator(char::from(*byte)))
            {
                joined.push(std::path::MAIN_SEPARATOR_STR);
            }
            joined.push(rest);
            PathBuf::from(joined)
        }
    } else {
        PathBuf::from(path)
    };
    if !target.is_absolute() {
        return Err("enter an absolute path or a path starting with ~/");
    }
    Ok(target)
}

/// The reply for `target`, from one metadata call that follows links.
fn reply(target: &Path) -> Value {
    match std::fs::metadata(target) {
        Ok(metadata) if metadata.is_file() => {
            json!({"schema": SCHEMA, "exists": true, "kind": "file", "size": metadata.len()})
        }
        Ok(metadata) if metadata.is_dir() => {
            json!({"schema": SCHEMA, "exists": true, "kind": "directory", "size": null})
        }
        Ok(_) => json!({"schema": SCHEMA, "exists": true, "kind": "other", "size": null}),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            json!({"schema": SCHEMA, "exists": false, "kind": null, "size": null})
        }
        // A component that is not a folder: nothing exists at the path.
        Err(error) if error.raw_os_error() == Some(not_a_directory()) => {
            json!({"schema": SCHEMA, "exists": false, "kind": null, "size": null})
        }
        Err(_) => json!({"schema": SCHEMA, "exists": null, "kind": null, "size": null}),
    }
}

#[cfg(unix)]
fn not_a_directory() -> i32 {
    20 // ENOTDIR
}
#[cfg(windows)]
fn not_a_directory() -> i32 {
    267 // ERROR_DIRECTORY
}

fn path_argument(args: &[String]) -> Result<&str> {
    match args {
        [flag, path] if flag == "--path" => Ok(path),
        _ => Err(Error::new(2, USAGE)),
    }
}

/// The command: prints the reply and exits 0, or exits 1 for a path it
/// will not inspect and 2 for a malformed call.
pub fn command(args: &[String], dry: bool) -> Result<i32> {
    // A forwarded --dry-run arrives as a trailing argument.
    let (args, dry) = match args.split_last() {
        Some((last, rest)) if last == "--dry-run" => (rest, true),
        _ => (args, dry),
    };
    let path = path_argument(args)?;
    if dry {
        return Err(Error::new(1, "path-info only reads; it has no --dry-run"));
    }
    let home = crate::platform::home_dir();
    let target = expanded(path, home.as_deref()).map_err(|message| Error::new(1, message))?;
    println!("{}", reply(&target));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\Users\example"
        } else {
            "/home/example"
        })
    }

    #[test]
    fn a_path_is_absolute_or_under_home() {
        let home = home();
        assert_eq!(expanded("~", Some(&home)), Ok(home.clone()));
        assert_eq!(
            expanded("~/notes/plan.md", Some(&home)),
            Ok(home.join("notes/plan.md"))
        );
        assert_eq!(
            expanded("~//notes", Some(&home)),
            Ok(home.join("notes")),
            "further separators stay under home"
        );
        // A drive or root after ~/ is text under home, never a new start (zerus-fqsli).
        let drive = expanded(r"~/C:\other", Some(&home)).unwrap();
        assert!(drive.starts_with(&home), "{drive:?}");
        assert_eq!(
            drive.as_os_str(),
            format!("{}{}C:\\other", home.display(), std::path::MAIN_SEPARATOR).as_str()
        );
        let slash_home = PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" });
        assert_eq!(
            expanded("~/srv", Some(&slash_home)),
            Ok(slash_home.join("srv")),
            "no doubled separator after a root home"
        );
        assert!(expanded("notes/plan.md", Some(&home)).is_err());
        assert!(expanded("~user/plan.md", Some(&home)).is_err());
        assert!(expanded("", Some(&home)).is_err());
        assert!(expanded("/srv/a\nb", Some(&home)).is_err());
        assert!(expanded(&format!("/{}", "a".repeat(MAX_PATH)), Some(&home)).is_err());
        assert!(expanded("~/x", None).is_err(), "no home folder");
    }

    #[test]
    fn the_reply_names_a_file_and_its_size_a_folder_or_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, b"twelve bytes").unwrap();
        assert_eq!(
            reply(&file),
            json!({"schema": 1, "exists": true, "kind": "file", "size": 12})
        );
        assert_eq!(
            reply(dir.path()),
            json!({"schema": 1, "exists": true, "kind": "directory", "size": null})
        );
        let missing = json!({"schema": 1, "exists": false, "kind": null, "size": null});
        assert_eq!(reply(&dir.path().join("gone.md")), missing);
        assert_eq!(
            reply(&file.join("below-a-file")),
            missing,
            "a file is no folder"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_path_this_account_cannot_inspect_is_unknown_not_missing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("plan.md"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let answer = reply(&locked.join("plan.md"));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Root can read anything; elsewhere the folder hides the file.
        if answer["exists"] != json!(true) {
            assert_eq!(
                answer,
                json!({"schema": 1, "exists": null, "kind": null, "size": null})
            );
        }
    }

    #[test]
    fn the_call_takes_exactly_one_path() {
        assert_eq!(
            path_argument(&["--path".into(), "/srv/a".into()]).unwrap(),
            "/srv/a"
        );
        for args in [
            vec![],
            vec!["/srv/a".to_owned()],
            vec!["--path".to_owned()],
            vec!["--path".into(), "/a".into(), "/b".into()],
        ] {
            assert_eq!(path_argument(&args).unwrap_err().code, 2, "{args:?}");
        }
    }
}
