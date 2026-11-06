//! Kernel self-image freshness for the closed original program-stop helper.
//! This is NOT selected-CLI authentication. Its native parent must verify its
//! frozen image BEFORE binding the original child and releasing the envelope;
//! canonical probe/registration/validate and every other guard remain required.
//! No Peer/Reference, caller-selected object or role is constructed here.
use super::*;
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
};

const REQUIRED: i32 =
    libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
const IMAGE_BYTES: u64 = 256 << 20;
const COMMAND: [&str; 3] = ["worktrees", "program-stop", "--json"];
const CLEANUP_COMMAND: [&str; 3] = ["worktrees", "program-cleanup", "--json"];
#[derive(PartialEq, Eq)]
struct Object {
    device: u64,
    inode: u64,
    length: u64,
    changed: (i64, i64),
    modified: (i64, i64),
    mode: u32,
}
fn object(file: &File) -> Result<Object> {
    let m = file.metadata()?;
    ensure!(
        m.is_file()
            && m.ino() != 0
            && m.len() > 0
            && m.len() <= IMAGE_BYTES
            && m.mode() & 0o111 != 0
            && m.uid() == unsafe { libc::geteuid() },
        "original program helper image unavailable"
    );
    let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
    ensure!(
        seals >= 0 && seals & REQUIRED == REQUIRED,
        "original program helper image is not immutable"
    );
    let mut magic = [0; 4];
    use std::os::unix::fs::FileExt;
    ensure!(
        file.read_at(&mut magic, 0)? == 4 && &magic == b"\x7fELF",
        "original program helper must be native"
    );
    Ok(Object {
        device: m.dev(),
        inode: m.ino(),
        length: m.len(),
        changed: (m.ctime(), m.ctime_nsec()),
        modified: (m.mtime(), m.mtime_nsec()),
        mode: m.mode(),
    })
}
fn kernel_self() -> Result<File> {
    // Deliberately follow ONLY this fixed kernel link. No path/FD/PID enters
    // from an envelope; memfd display names and installed names are irrelevant.
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open("/proc/self/exe")?)
}
fn exact_command(args: &[String]) -> bool {
    args.iter().map(String::as_str).eq(COMMAND)
}
fn deadline(until: Instant) -> Result<()> {
    ensure!(
        Instant::now() < until,
        "original program helper proof expired"
    );
    Ok(())
}
pub(super) struct OwnImage {
    pid: u32,
    identity: platform::ProcessIdentity,
    file: File,
    object: Object,
}
impl OwnImage {
    /// The only production constructor takes no identity/image selector.
    /// It is usable only in the exact new stdio entry, not frontend-request.
    pub(super) fn capture(until: Instant) -> Result<Self> {
        Self::capture_command(until, &COMMAND)
    }
    pub(super) fn capture_cleanup(until: Instant) -> Result<Self> {
        Self::capture_command(until, &CLEANUP_COMMAND)
    }
    // Expected argv is selected only by the two fixed native entry points.
    // No request/body field can select self mode, PID, FD, path or image.
    fn capture_command(until: Instant, command: &'static [&'static str; 3]) -> Result<Self> {
        deadline(until)?;
        let args = std::env::args_os()
            .skip(1)
            .take(COMMAND.len() + 1)
            .map(|arg| {
                arg.into_string()
                    .map_err(|_| anyhow::anyhow!("unexpected program-stop helper command"))
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            if command == &COMMAND {
                exact_command(&args)
            } else {
                args.iter().map(String::as_str).eq(command.iter().copied())
            },
            "unexpected program-stop helper command"
        );
        let pid = std::process::id();
        let identity =
            platform::process_identity(pid).context("original helper process unavailable")?;
        let file = kernel_self()?;
        let expected = object(&file)?;
        let own = Self {
            pid,
            identity,
            file,
            object: expected,
        };
        own.check(until)?;
        Ok(own)
    }
    /// Caller supplies its ORIGINAL proof deadline, or after one-shot proof
    /// consumption its separately bounded operation deadline. This method
    /// observes object freshness only and never creates/renews either permit.
    pub(super) fn check(&self, until: Instant) -> Result<()> {
        deadline(until)?;
        ensure!(
            self.pid == std::process::id()
                && platform::process_identity(self.pid).as_ref() == Some(&self.identity),
            "original helper changed"
        );
        ensure!(
            object(&self.file)? == self.object && object(&kernel_self()?)? == self.object,
            "original helper image changed"
        );
        ensure!(
            platform::process_identity(self.pid).as_ref() == Some(&self.identity),
            "original helper changed"
        );
        deadline(until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::FromRawFd;
    fn image(seals: i32) -> File {
        const MFD_EXEC: u32 = 0x0010;
        let fd = unsafe {
            libc::memfd_create(
                c"same-display-name".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING | MFD_EXEC,
            )
        };
        assert!(fd >= 0);
        let mut file = unsafe { File::from_raw_fd(fd) };
        use std::io::Write;
        let mut bytes = Vec::new();
        File::open("/bin/true")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        file.write_all(&bytes).unwrap();
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, seals) }, 0);
        file
    }
    #[test]
    fn only_the_exact_closed_command_can_select_the_own_helper_case() {
        assert!(exact_command(&COMMAND.map(str::to_owned)));
        for command in [
            vec!["desktop", "frontend-request", "--json"],
            vec!["worktrees", "program-stop"],
            vec!["worktrees", "program-stop", "--json", "extra"],
            vec![
                "--expect-machine",
                "anything",
                "worktrees",
                "program-stop",
                "--json",
            ],
            vec![
                "--desktop-helper-rendezvous-v1",
                "worktrees",
                "program-stop",
                "--json",
            ],
        ] {
            assert!(!exact_command(
                &command.into_iter().map(str::to_owned).collect::<Vec<_>>()
            ));
        }
        assert!(
            OwnImage::capture(Instant::now() + WAIT).is_err(),
            "a unit-test argv is never the stdio entry"
        );
    }
    #[test]
    fn mechanism_requires_every_seal_and_same_named_objects_stay_distinct() {
        // Mechanism only: these are not THIS process's image, selected CLI
        // proof or a genuine canonical frontend positive.
        for missing in [
            libc::F_SEAL_WRITE,
            libc::F_SEAL_GROW,
            libc::F_SEAL_SHRINK,
            libc::F_SEAL_SEAL,
        ] {
            assert!(object(&image(REQUIRED & !missing)).is_err());
        }
        assert!(object(&image(libc::F_SEAL_FUTURE_WRITE)).is_err());
        let first = image(REQUIRED);
        let second = image(REQUIRED);
        assert!(object(&first).unwrap() != object(&second).unwrap());
        let fake = OwnImage {
            pid: std::process::id(),
            identity: platform::process_identity(std::process::id()).unwrap(),
            object: object(&first).unwrap(),
            file: first,
        };
        assert!(
            fake.check(Instant::now() + WAIT).is_err(),
            "a retained other image is not our kernel exe"
        );
        assert!(fake.check(Instant::now()).is_err());
    }
}
