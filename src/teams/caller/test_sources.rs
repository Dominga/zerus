//! Test-only kernel inputs. They run the same admission and ancestry walk as
//! the proof's own model tests; no caller result or authority is injected.
use super::*;

pub(super) fn selected(name: &str) -> bool {
    // A late-admission control changes only this test's synthetic kernel read.
    // It never injects a caller result or changes the ordinary task check.
    if name == "opaque" && std::env::var("HGS_TEAMS_TEST_CONTEXT").as_deref() == Ok("opaque_after")
    {
        return std::env::var_os("HGS_TEAMS_TEST_CONTEXT_AFTER")
            .is_some_and(|path| std::path::Path::new(&path).is_file());
    }
    std::env::var("HGS_TEAMS_TEST_CONTEXT").as_deref() == Ok(name)
}

pub(super) fn called() {
    if let Some(path) = std::env::var_os("HGS_TEAMS_TEST_PROOF_READS") {
        use std::io::Write;
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)
                .unwrap(),
            "proof"
        )
        .unwrap();
    }
}
pub(super) fn opaque_inspect(pid: u32) -> Result<ancestry::Inspection, Problem> {
    Ok(match pid {
        2..=4 => ancestry::Inspection::Own(ancestry::Process {
            parent: pid - 1,
            birth: Some("fixture-birth".into()),
        }),
        _ => ancestry::Inspection::Foreign,
    })
}

pub(super) fn opaque_environment(_: u32) -> std::io::Result<Vec<u8>> {
    Err(super::withheld())
}

#[cfg(target_os = "linux")]
pub(super) fn namespace_input() -> std::io::Result<(tempfile::TempDir, std::os::fd::OwnedFd)> {
    use std::os::unix::fs::OpenOptionsExt;
    let root = tempfile::tempdir()?;
    let link = root.path().join("namespace");
    std::os::unix::fs::symlink("user:[42]", &link)?;
    let namespace = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW)
        .open(link)?;
    Ok((root, namespace.into()))
}
