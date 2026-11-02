//! A foreign UID is not an ancestry permission. Only the native system login
//! bridge and PID 1 boundary have defined roles; own-user nodes keep all checks.
use anyhow::{ensure, Result};
use std::{collections::BTreeSet, time::Instant};

#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
pub(super) use native::inspect;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Unique {
    executable: [u8; 16],
    // Kernel process generations, not wall-clock timestamps or bare PIDs.
    birth: u64,
    parent_birth: u64,
    version: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SystemImage {
    device: u64,
    inode: u64,
    uid: u32,
    mode: u32,
    len: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl SystemImage {
    fn protected(&self) -> bool {
        self.uid == 0 && self.mode & 0o170000 == 0o100000 && self.mode & 0o022 == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    pid: u32,
    parent: u32,
    group: u32,
    session: u32,
    status: u32,
    command: [u8; 16],
    flags: u32,
    // Effective, real and saved UID/GID; never infer a foreign owner on error.
    uids: [u32; 3],
    gids: [u32; 3],
    unique: Unique,
    image: String,
    system_image: Option<SystemImage>,
}

#[derive(Debug, PartialEq, Eq)]
enum Role {
    Own,
    Login,
    Init,
}

fn classify(value: &Snapshot, owner: u32) -> Result<Role> {
    ensure!(
        owner != 0
            && value.pid != 0
            && value.unique.birth != 0
            && (1..=6).contains(&value.status)
            && value.status != 5,
        "macOS ancestry identity unavailable"
    );
    if value.uids[0] == owner {
        return Ok(Role::Own);
    }
    ensure!(
        value
            .system_image
            .as_ref()
            .is_some_and(SystemImage::protected),
        "unverified foreign macOS ancestor"
    );
    if value.pid == 1
        && value.parent == 0
        && value.group == 1
        && value.session == 1
        && value.uids == [0; 3]
        && value.gids == [0; 3]
        && value.unique.parent_birth == 0
        && value.image == "/sbin/launchd"
    {
        return Ok(Role::Init);
    }
    if value.pid > 1
        && value.parent > 1
        && value.group == value.pid
        && value.session == value.pid
        && value.uids == [0, owner, 0]
        && value.unique.parent_birth != 0
        && value.image == "/usr/bin/login"
    {
        return Ok(Role::Login);
    }
    anyhow::bail!("unsupported foreign macOS ancestor")
}

fn walk<T>(
    mut pid: u32,
    owner: u32,
    until: Instant,
    mut read: impl FnMut(u32) -> Result<Snapshot>,
    mut own: impl FnMut(&Snapshot) -> Result<T>,
) -> Result<Vec<T>> {
    let mut seen = BTreeSet::new();
    let mut retained: Vec<Snapshot> = Vec::new();
    let mut owned = Vec::new();
    let mut login_parent = None;
    loop {
        ensure!(
            pid != 0 && seen.len() < super::ANCESTORS && Instant::now() < until && seen.insert(pid),
            "macOS frontend ancestry unavailable"
        );
        let value = read(pid)?;
        ensure!(value.pid == pid, "macOS ancestor PID changed");
        let role = classify(&value, owner)?;
        if let Some((parent, birth)) = login_parent.take() {
            ensure!(
                role == Role::Own && value.pid == parent && value.unique.birth == birth,
                "macOS login parent identity changed"
            );
        }
        match role {
            Role::Own => owned.push(own(&value)?),
            Role::Login => {
                let child = retained.last().filter(|v| v.uids[0] == owner);
                ensure!(
                    child
                        .is_some_and(|v| v.parent == value.pid
                            && v.unique.parent_birth == value.unique.birth),
                    "macOS login original child-parent edge changed"
                );
                login_parent = Some((value.parent, value.unique.parent_birth));
            }
            Role::Init => {
                ensure!(
                    !owned.is_empty(),
                    "macOS frontend ancestry has no owned caller"
                );
                retained.push(value);
                break;
            }
        }
        pid = value.parent;
        retained.push(value);
    }
    // Recheck every original short/unique/image identity, including the bridge
    // and its Terminal parent. A copied path, PID reuse or reparent is no proof.
    for value in retained {
        ensure!(
            Instant::now() < until,
            "macOS ancestry verification expired"
        );
        ensure!(read(value.pid)? == value, "macOS frontend ancestry changed");
    }
    Ok(owned)
}

#[cfg(test)]
mod tests;
