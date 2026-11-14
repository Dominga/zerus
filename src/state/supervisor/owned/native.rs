//! No helpers, shell, registry, saved PID adoption or policy repair.
use crate::platform::{self, FileIdentity};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, net::UnixStream},
    },
    path::{Path, PathBuf},
};

struct Held {
    path: PathBuf,
    file: File,
    identity: FileIdentity,
    directory: bool,
}
impl Held {
    fn new(path: &Path, directory: bool) -> io::Result<Self> {
        let path = platform::canonical(path)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)?;
        let meta = file.metadata()?;
        if (directory && !meta.is_dir()) || (!directory && !meta.is_file()) {
            return Err(io::Error::other("original source type"));
        }
        let identity = FileIdentity::of(&file)?;
        Ok(Self {
            path,
            file,
            identity,
            directory,
        })
    }
    fn current(&self) -> bool {
        let same = |v: Option<FileIdentity>| {
            v.is_some_and(|v| {
                if self.directory {
                    v.device == self.identity.device && v.inode == self.identity.inode
                } else {
                    v == self.identity
                }
            })
        };
        same(FileIdentity::of(&self.file).ok()) && same(platform::file_identity(&self.path))
    }
    fn wire(&self) -> Value {
        json!({"path":self.path,"device":self.identity.device,"inode":self.identity.inode,
            "modified":self.identity.modified,"nanos":self.identity.modified_nanos})
    }
}
pub(super) fn profile() -> bool {
    !std::env::vars_os().any(|(key, _)| {
        let key = key.to_string_lossy();
        key == "LD_PRELOAD"
            || key == "LD_AUDIT"
            || key == "LD_LIBRARY_PATH"
            || key.starts_with("DYLD_")
    })
}

pub(super) struct Source {
    state: Held,
    config: Held,
    image: Held,
    birth: String,
}
impl Source {
    pub(super) fn capture(state: &Path, config: &Path) -> io::Result<Self> {
        let image = Held::new(&std::env::current_exe()?, false)?;
        if platform::image_identity() != Some(image.identity) {
            return Err(io::Error::other("selected image changed"));
        }
        let birth =
            birth(std::process::id()).ok_or_else(|| io::Error::other("owner birth unavailable"))?;
        Ok(Self {
            state: Held::new(state, true)?,
            config: Held::new(config, true)?,
            image,
            birth,
        })
    }
    pub(super) fn current(&self) -> bool {
        self.state.current()
            && self.config.current()
            && self.image.current()
            && platform::image_identity() == Some(self.image.identity)
            && birth(std::process::id()).is_some_and(|birth| birth == self.birth)
    }
    pub(super) fn birth(&self) -> &str {
        &self.birth
    }
    pub(super) fn wire(&self) -> Value {
        json!({"state":self.state.wire(),"configuration":self.config.wire(),"image":self.image.wire()})
    }
    pub(super) fn peer(&self, stream: &UnixStream) -> bool {
        let Some(pid) = peer_pid(stream) else {
            return false;
        };
        // Read-only observation is restricted to this same selected CLI image,
        // checked from the native peer, never a renderer- or JSON-supplied PID.
        let Some(before) = birth(pid) else {
            return false;
        };
        image_of(pid) == Some(self.image.identity)
            && birth(pid).is_some_and(|after| after == before)
    }
}
pub(super) fn wait_policy() -> bool {
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    let base = (unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) == 0 })
        && action.sa_sigaction == libc::SIG_DFL
        && action.sa_flags & libc::SA_NOCLDWAIT == 0;
    #[cfg(target_os = "linux")]
    {
        let mut flag = 0;
        base && unsafe { libc::prctl(libc::PR_GET_CHILD_SUBREAPER, &mut flag, 0, 0, 0) } == 0
            && flag == 1
    }
    #[cfg(target_os = "macos")]
    {
        base
    }
}
#[cfg(target_os = "macos")]
pub(super) fn admit_root() -> io::Result<()> {
    if !wait_policy() {
        return Err(io::Error::other("original wait policy unavailable"));
    }
    Ok(())
}
#[cfg(target_os = "macos")]
pub(super) fn root_ready(pid: u32) -> io::Result<bool> {
    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    if unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info.si_pid != 0)
}
pub(super) fn peer_pid(stream: &UnixStream) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let mut c: libc::ucred = unsafe { std::mem::zeroed() };
        let mut n = std::mem::size_of_val(&c) as libc::socklen_t;
        let ok = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut c as *mut libc::ucred).cast(),
                &mut n,
            )
        };
        (ok == 0
            && n as usize == std::mem::size_of_val(&c)
            && c.uid == unsafe { libc::geteuid() }
            && c.pid > 0)
            .then_some(c.pid as u32)
    }
    #[cfg(target_os = "macos")]
    {
        let (mut uid, mut gid) = (0, 0);
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0
            || uid != unsafe { libc::geteuid() }
        {
            return None;
        }
        let mut pid: libc::pid_t = 0;
        let mut n = std::mem::size_of_val(&pid) as libc::socklen_t;
        let ok = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut n,
            )
        };
        (ok == 0 && n as usize == std::mem::size_of_val(&pid) && pid > 0).then_some(pid as u32)
    }
}

#[cfg(target_os = "linux")]
pub(super) fn image_of(pid: u32) -> Option<FileIdentity> {
    platform::file_identity(Path::new(&format!("/proc/{pid}/exe")))
}
#[cfg(target_os = "macos")]
pub(super) fn image_of(pid: u32) -> Option<FileIdentity> {
    // Read the mapped native vnode, not a possibly replaced executable path.
    #[repr(C)]
    struct Region {
        info: [u64; 12],
        vnode: libc::vnode_info_path,
    }
    const _: () = assert!(std::mem::size_of::<Region>() == 1272);
    let mut expected = [0u8; 4096];
    let n = unsafe {
        libc::proc_pidpath(
            pid as i32,
            expected.as_mut_ptr().cast(),
            expected.len() as u32,
        )
    };
    if n <= 0 {
        return None;
    }
    let end = expected.iter().position(|b| *b == 0)?;
    let expected = &expected[..end];
    let mut address = 0;
    for _ in 0..64 {
        let mut region: Region = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::proc_pidinfo(
                pid as i32,
                8,
                address,
                (&mut region as *mut Region).cast(),
                std::mem::size_of::<Region>() as i32,
            )
        } != std::mem::size_of::<Region>() as i32
        {
            return None;
        }
        let path: Vec<u8> = region
            .vnode
            .vip_path
            .iter()
            .flatten()
            .map(|b| *b as u8)
            .take_while(|b| *b != 0)
            .collect();
        if path == expected {
            let stat = region.vnode.vip_vi.vi_stat;
            return Some(FileIdentity {
                device: stat.vst_dev as u64,
                inode: stat.vst_ino,
                modified: stat.vst_mtime,
                modified_nanos: stat.vst_mtimensec as i64,
            });
        }
        let next = region.info[10].checked_add(region.info[11])?;
        if next <= address {
            return None;
        }
        address = next;
    }
    None
}

#[cfg(target_os = "linux")]
pub(super) fn birth(pid: u32) -> Option<String> {
    use std::{io::Read, os::unix::fs::MetadataExt};
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(format!("/proc/{pid}/stat"))
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    let mut raw = Vec::new();
    file.take(4097).read_to_end(&mut raw).ok()?;
    if raw.len() > 4096 {
        return None;
    }
    let text = std::str::from_utf8(&raw).ok()?;
    let (_, fields) = text.rsplit_once(") ")?;
    let fields: Vec<_> = fields.split_ascii_whitespace().take(20).collect();
    if matches!(fields.first().copied(), Some("Z" | "X")) {
        return None;
    }
    Some(fields.get(19)?.to_string())
}
#[cfg(target_os = "macos")]
pub(super) fn birth(pid: u32) -> Option<String> {
    platform::process_identity(pid).map(|i| i.birth)
}
