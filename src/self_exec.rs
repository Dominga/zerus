//! Production helper entry points require the CLI's real `main` dispatcher.
//! libtest replaces it and interprets helper argv as test filters instead.
use std::{io, path::PathBuf};

pub(crate) const TEST_IMAGE: &str = "A unit-test image cannot launch a production hgs helper";

/// Check before manager activation or any other work leading to self-exec.
/// Explicit test subprocesses still use their exact test filters directly.
pub(crate) fn ensure() -> io::Result<()> {
    if cfg!(test) {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, TEST_IMAGE))
    } else {
        Ok(())
    }
}

pub(crate) fn current() -> io::Result<PathBuf> {
    ensure()?;
    std::env::current_exe()
}

/// Exercise a launcher only in an exact-filter child with private stores and
/// an independent descendant supervisor, even if a guard is later regressed.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn private_probe(test: &str, probe: impl FnOnce()) {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    const CHILD: &str = "HGS_SELF_EXEC_PROBE";
    if std::env::var(CHILD).ok().as_deref() == Some(test) {
        probe();
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for name in ["tmp", "runtime"] {
        fs::create_dir(root.join(name)).unwrap();
        fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let called = root.join("manager-called");
    let manager = bin.join("systemctl");
    fs::write(
        &manager,
        "#!/bin/sh\n: > \"$HOME/manager-called\"\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&manager, fs::Permissions::from_mode(0o700)).unwrap();
    let units = root.join(".config/systemd/user");
    fs::create_dir_all(&units).unwrap();
    for unit in ["hgs-voice.service", "hgs-recovery.service"] {
        fs::write(units.join(unit), "synthetic fixture\n").unwrap();
    }
    let status = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/bounded_process.py"
        ))
        .args(["--seconds", "10", "--max-processes", "16", "--"])
        .arg(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1"])
        .env_clear()
        .env("LANG", "C.UTF-8")
        .env(CHILD, test)
        .env("HOME", &root)
        .env("TMPDIR", root.join("tmp"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join(".config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HGS_CONFIG_DIR", root.join(".config/hgs"))
        .env("HGS_STATE_DIR", root.join("state/hgs"))
        .env(
            "PATH",
            std::env::join_paths([bin, "/usr/bin".into(), "/bin".into()]).unwrap(),
        )
        .status()
        .unwrap();
    assert!(
        status.success(),
        "isolated self-exec probe failed: {status}"
    );
    assert!(!called.exists(), "test image activated a service manager");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libtest_never_returns_a_production_helper_image() {
        assert_eq!(
            ensure().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(current().unwrap_err().to_string(), TEST_IMAGE);
    }
}
