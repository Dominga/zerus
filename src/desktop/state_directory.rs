//! Repair only the selected state directory's legacy Unix mode. No ownership,
//! service, journal, child entry or missing directory is created or adopted.
use super::store::Location;
use crate::config::Config;
use anyhow::Result;

pub(super) fn prepare(config: &Config, location: &Location) -> Result<()> {
    #[cfg(unix)]
    {
        tighten(
            &config.state,
            location.root.parent().expect("state directory"),
        )?;
    }
    #[cfg(not(unix))]
    {
        use anyhow::Context;
        let _ = config;
        // Windows ACLs are never repaired by this Unix compatibility migration.
        // WhiteWh fork, task 99: but the state folder may inherit a sandbox
        // group's read ACE (Codex adds one to ~/.local), and the service
        // would then refuse to start before `Location::prepare` could repair
        // it. A folder this user owns that only Windows left too open is made
        // private in place, as there; a missing one is not created here, and
        // one that stays open is still refused.
        for path in [location.root.parent().context("missing state folder")?, &location.root] {
            match crate::platform::verify_private(path, crate::platform::Private::Directory) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    // Task 176: a folder that stays open is refused with its
                    // owner or access entry, so the desktop can show why.
                    if !crate::private_stores::ensure(path, crate::platform::Private::Directory)? {
                        return Err(super::store::not_private(path, None));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn tighten(selected: &std::path::Path, canonical: &std::path::Path) -> Result<()> {
    use anyhow::ensure;
    use std::{
        fs,
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    };
    let named = match fs::symlink_metadata(selected) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if named.file_type().is_symlink() {
        // Location/configuration already resolve aliases. Preserve an existing
        // PRIVATE target without chmod, but never repair through this alias.
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(canonical)?;
        let held = file.metadata()?;
        let resolved = fs::metadata(selected)?;
        let current = fs::symlink_metadata(canonical)?;
        ensure!(
            resolved.is_dir()
                && current.is_dir()
                && (resolved.dev(), resolved.ino()) == (held.dev(), held.ino())
                && (current.dev(), current.ino()) == (held.dev(), held.ino())
                && crate::platform::verify_private_directory(&file)?,
            "desktop state alias must name the same already-private directory"
        );
        return Ok(());
    }
    ensure!(
        named.is_dir() && named.uid() == unsafe { libc::geteuid() },
        "desktop state directory must belong to this user and not be a symlink"
    );
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(selected)?;
    let original = file.metadata()?;
    ensure!(
        original.is_dir() && original.uid() == unsafe { libc::geteuid() },
        "desktop state directory must belong to this user"
    );
    ensure!(
        (named.dev(), named.ino()) == (original.dev(), original.ino()),
        "desktop state directory changed"
    );
    let unchanged = || -> std::io::Result<bool> {
        let same = |path| -> std::io::Result<bool> {
            let now = fs::symlink_metadata(path)?;
            Ok(now.is_dir() && now.dev() == original.dev() && now.ino() == original.ino())
        };
        Ok(same(selected)? && same(canonical)?)
    };
    ensure!(unchanged()?, "desktop state directory changed");
    if original.mode() & 0o077 != 0 {
        // File::set_permissions uses fchmod: never chmod a newly substituted
        // pathname. Only REMOVE access, only on this already-owned directory.
        file.set_permissions(fs::Permissions::from_mode(original.mode() & 0o700))?;
    }
    ensure!(
        unchanged()? && crate::platform::verify_private_directory(&file)?,
        "desktop state directory changed"
    );
    Ok(())
}

/// Task 176 (owner, 2026-11-07): Codex grants its sandbox group read access on
/// `~/.local`, and the session state folder below inherits it. The read-only
/// check names the folder and that access entry; the service start's repair
/// makes the owned folder private in place; a refusal that stays says why.
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::platform::{self, Private};
    use std::{fs, path::Path, process::Command};

    fn icacls(path: &Path, args: &[&str]) {
        let output = Command::new("icacls").arg(path).args(args).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    }

    fn config(root: &Path) -> Config {
        Config {
            home: root.to_string_lossy().into_owned(),
            dir: root.join("config"),
            state: root.join(".local").join("sessions"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        }
    }

    #[test]
    fn an_inherited_sandbox_read_is_named_then_repaired_by_the_service_start() {
        let temp = tempfile::tempdir().unwrap();
        let root = platform::canonical(temp.path()).unwrap();
        crate::state::with_test_state_root(&root, || {
            let local = root.join(".local");
            fs::create_dir(&local).unwrap();
            // As Codex leaves ~/.local: its group may read, and folders below inherit it.
            icacls(&local, &["/grant", "*S-1-5-32-545:(OI)(CI)RX"]);
            let sessions = local.join("sessions");
            fs::create_dir(&sessions).unwrap();
            assert!(!platform::verify_private(&sessions, Private::Directory).unwrap(), "the state folder inherited the read");
            let location = Location { root: sessions.join("desktop"), configuration: "c".into() };
            // The read-only path refuses, naming the folder, the entry and the repair.
            let error = format!("{:#}", location.check_existing().unwrap_err());
            assert!(error.starts_with(super::super::store::NOT_PRIVATE), "{error}");
            assert!(error.contains(&sessions.display().to_string()), "{error}");
            assert!(error.contains("S-1-5-32-545") && error.contains("read and execute (RX)"), "{error}");
            assert!(error.contains("inherited from its parent folder"), "{error}");
            assert!(error.contains("hgs desktop prepare-state --json"), "{error}");
            assert!(!sessions.join("desktop").exists(), "the read-only path creates nothing");
            // The service start's repair makes the owned folder private in place.
            prepare(&config(&root), &location).unwrap();
            assert!(platform::verify_private(&sessions, Private::Directory).unwrap());
            location.check_existing().unwrap();
            assert!(platform::describe_private(&sessions, Private::Directory).unwrap().is_none());
            // An explicit grant reads as such, without "inherited".
            let open = root.join("open");
            fs::create_dir(&open).unwrap();
            icacls(&open, &["/inheritance:r", "/grant:r", &format!("*{}:(OI)(CI)F", platform::user_sid_string().unwrap())]);
            icacls(&open, &["/grant", "*S-1-5-32-545:(OI)(CI)M"]);
            let reason = platform::describe_private(&open, Private::Directory).unwrap().unwrap();
            assert!(reason.contains("S-1-5-32-545") && reason.contains("modify (M)") && !reason.contains("inherited"), "{reason}");
        });
    }

    #[test]
    fn a_refusal_the_repair_cannot_fix_says_why() {
        let temp = tempfile::tempdir().unwrap();
        let root = platform::canonical(temp.path()).unwrap();
        crate::state::with_test_state_root(&root, || {
            let local = root.join(".local");
            fs::create_dir(&local).unwrap();
            // A file where the state folder belongs: no repair makes it a folder.
            fs::write(local.join("sessions"), b"x").unwrap();
            let location = Location { root: local.join("sessions").join("desktop"), configuration: "c".into() };
            let error = format!("{:#}", prepare(&config(&root), &location).unwrap_err());
            assert!(error.starts_with(super::super::store::NOT_PRIVATE) && error.contains("it is not a folder"), "{error}");
            let error = format!("{:#}", location.check_existing().unwrap_err());
            assert!(error.contains("it is not a folder"), "{error}");
        });
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, MetadataExt, PermissionsExt},
    };

    #[test]
    fn repair_is_owned_directory_only_nonrecursive_and_does_not_create() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("sessions");
        tighten(&state, &state).unwrap();
        assert!(!state.exists());
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o755)).unwrap();
        let child = state.join("keep");
        fs::write(&child, b"original").unwrap();
        fs::set_permissions(&child, fs::Permissions::from_mode(0o644)).unwrap();
        let before = fs::metadata(&state).unwrap();
        tighten(&state, &state).unwrap();
        tighten(&state, &state).unwrap();
        let after = fs::metadata(&state).unwrap();
        assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
        assert_eq!(after.mode() & 0o777, 0o700);
        assert_eq!(fs::read(&child).unwrap(), b"original");
        assert_eq!(fs::metadata(&child).unwrap().mode() & 0o777, 0o644);
        assert_eq!(fs::read_dir(&state).unwrap().count(), 1);
    }

    #[test]
    fn repair_refuses_symlink_file_foreign_owner_and_mismatched_identity() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("sessions");
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o755)).unwrap();
        let link = temp.path().join("link");
        symlink(&state, &link).unwrap();
        assert!(tighten(&link, &state).is_err());
        fs::remove_file(&link).unwrap();
        symlink(temp.path().join("missing"), &link).unwrap();
        assert!(tighten(&link, &state).is_err());
        let file = temp.path().join("file");
        fs::write(&file, b"keep").unwrap();
        assert!(tighten(&file, &file).is_err());
        assert!(tighten(&state, temp.path()).is_err());
        assert_eq!(fs::metadata(&state).unwrap().mode() & 0o777, 0o755);
        // A real root-owned directory, never chmodded by this non-root control.
        if unsafe { libc::geteuid() } != fs::metadata("/").unwrap().uid() {
            let before = fs::metadata("/").unwrap().mode();
            assert!(tighten(std::path::Path::new("/"), std::path::Path::new("/")).is_err());
            assert_eq!(fs::metadata("/").unwrap().mode(), before);
        }
    }

    #[test]
    fn an_already_private_alias_is_nonmutating_but_never_authorizes_repair() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("sessions");
        crate::platform::private_dir(&state).unwrap();
        let link = temp.path().join("alias");
        symlink(&state, &link).unwrap();
        let before = fs::metadata(&state).unwrap();
        tighten(&link, &state).unwrap();
        let after = fs::metadata(&state).unwrap();
        assert_eq!(
            (
                before.dev(),
                before.ino(),
                before.mode(),
                before.ctime(),
                before.ctime_nsec()
            ),
            (
                after.dev(),
                after.ino(),
                after.mode(),
                after.ctime(),
                after.ctime_nsec()
            )
        );
        assert!(tighten(&link, temp.path()).is_err());
        fs::set_permissions(&state, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(tighten(&link, &state).is_err());
        assert_eq!(fs::metadata(&state).unwrap().mode() & 0o777, 0o755);
    }
}
