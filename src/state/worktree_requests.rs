//! Private, opt-in creation receipts. An interrupted reservation is never replayed.
use super::*;
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fs::{File, OpenOptions};

thread_local! {
    static CANCEL: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static EFFECTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    #[cfg(unix)]
    static LIFELINE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(super) fn reserve_checkout() -> Result<()> {
    if cancelled() {
        return Err("Worktree creation stopped before checkout".into());
    }
    let path = CANCEL.with(|slot| slot.borrow().clone());
    if let Some(path) = path {
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Invalid worktree reservation")?;
        let mut value = read(id)?.ok_or("Missing worktree reservation")?;
        value["effects_started"] = json!(true);
        atomic(&path.with_extension("json"), &value.to_string())?;
        EFFECTS.with(|slot| slot.set(true));
    }
    Ok(())
}

pub(super) fn cancelled() -> bool {
    let marker = CANCEL.with(|path| {
        path.borrow()
            .as_ref()
            .is_some_and(|p| p.try_exists().unwrap_or(true))
    });
    #[cfg(unix)]
    if LIFELINE.with(std::cell::Cell::get) {
        // The wrapper never sends bytes. EOF or unexpected input withdraws only
        // this worker; Git receives /dev/null and cannot retain this lifeline.
        let mut byte = 0u8;
        let result = unsafe { libc::read(0, (&mut byte as *mut u8).cast(), 1) };
        if result >= 0 {
            return true;
        }
        if !matches!(
            io::Error::last_os_error().kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
        ) {
            return true;
        }
    }
    marker
}

fn token(args: &[String]) -> Result<String> {
    let mut ids = args.windows(2).filter(|pair| pair[0] == "--request-id");
    let value = ids.next().ok_or("--request-id UUID is required")?[1].clone();
    if ids.next().is_some()
        || !uuid::Uuid::parse_str(&value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
    {
        return Err("--request-id needs a canonical non-nil UUID".into());
    }
    Ok(value)
}

fn folder() -> PathBuf {
    root().join("worktree-requests")
}
fn read(id: &str) -> Result<Option<Value>> {
    let path = folder().join(format!("{id}.json"));
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    use std::io::Read;
    file.by_ref()
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32768 {
        return Err("Worktree receipt exceeds its limit".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Unreadable worktree receipt")?;
    if value["schema"] != 1 || value["request_id"] != id || !value["status"].is_string() {
        return Err("Unsupported worktree receipt".into());
    }
    Ok(Some(value))
}

fn lock(id: &str, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .truncate(false);
    crate::platform::private_open(&mut options)
        .open(folder().join(format!("{id}.lock")))
        .map_err(|e| e.to_string())
}

fn snapshot(id: &str) -> Result<Value> {
    let Some(mut value) = read(id)? else {
        return Ok(json!({"schema":1,"request_id":id,"status":"not_found"}));
    };
    if value["status"] == "running" {
        // A free original lock proves the worker is gone, not that Git did nothing.
        if let Ok(file) = lock(id, false) {
            if file.try_lock_exclusive().is_ok() {
                value["status"] = json!("unconfirmed");
            }
        }
    }
    if value["status"] == "created" {
        let current = value["common_dir"]
            .as_str()
            .zip(value["path"].as_str())
            .and_then(|(common, path)| {
                super::worktree_lifecycle::owned(Path::new(common), Path::new(path))
            });
        if current.is_none() || current.as_ref() != Some(&value["creation_owner"]) {
            value["resource_error"]=json!("The prepared worktree ownership changed or cannot be verified. Inspect the retained checkout.");
        }
    }
    Ok(value)
}

pub(super) fn control(action: &str, args: &[String]) -> Result<Value> {
    let id = token(args)?;
    if args.len() != 3 || !args.iter().any(|a| a == "--json") {
        return Err("usage: hgs worktrees status|cancel --request-id UUID --json".into());
    }
    let value = snapshot(&id)?;
    if action == "cancel" && value["status"] == "running" {
        // This marker is scoped to a private original request. Only its own Git
        // group observes it; accepting this command is not proof of process exit.
        atomic(&folder().join(format!("{id}.cancel")), "cancel")?;
    }
    Ok(value)
}

#[cfg(not(unix))]
pub(super) fn create(args: &[String]) -> Result<Value> {
    create_direct(args)
}

fn create_direct(args: &[String]) -> Result<Value> {
    let id = token(args)?;
    let args: Vec<String> = args.iter().filter(|a| *a != "--receipt").cloned().collect();
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&args).map_err(|e| e.to_string())?)
    );
    private_dir(&folder())?;
    let guard = lock(&id, true)?;
    if guard.try_lock_exclusive().is_err() {
        return match read(&id)? {
            Some(value) if value["fingerprint"] == fingerprint => Ok(value),
            Some(_) => Err("Worktree request arguments do not match the original request".into()),
            None => Err("Worktree request is being reserved; check its original result".into()),
        };
    }
    if let Some(mut value) = read(&id)? {
        if value["fingerprint"] != fingerprint {
            return Err("Worktree request arguments do not match the original request".into());
        }
        if value["status"] == "running" {
            value["status"] = json!("unconfirmed");
        }
        return Ok(value);
    }
    let mut receipt = json!({"schema":1,"request_id":id,"fingerprint":fingerprint,
        "status":"running","started_at":now()});
    atomic(&folder().join(format!("{id}.json")), &receipt.to_string())?;
    CANCEL.with(|slot| *slot.borrow_mut() = Some(folder().join(format!("{id}.cancel"))));
    EFFECTS.with(|slot| slot.set(false));
    let result = super::worktrees::create(&args);
    CANCEL.with(|slot| *slot.borrow_mut() = None);
    match result {
        Ok(value) => {
            receipt["creation_owner"] = value["common_dir"]
                .as_str()
                .zip(value["path"].as_str())
                .and_then(|(common, path)| {
                    super::worktree_lifecycle::owned(Path::new(common), Path::new(path))
                })
                .unwrap_or(Value::Null);
            for (key, value) in value.as_object().ok_or("Invalid worktree result")? {
                receipt[key] = value.clone();
            }
        }
        Err(error) => {
            receipt["status"] = json!("failed");
            receipt["detail"] = json!(error.chars().take(4096).collect::<String>());
        }
    }
    receipt["finished_at"] = json!(now());
    receipt["effects_started"] = json!(EFFECTS.with(std::cell::Cell::get));
    atomic(&folder().join(format!("{id}.json")), &receipt.to_string())?;
    Ok(receipt)
}

#[cfg(unix)]
fn image_proof(identity: crate::platform::FileIdentity) -> String {
    format!(
        "{}:{}:{}:{}",
        identity.device, identity.inode, identity.modified, identity.modified_nanos
    )
}

#[cfg(unix)]
pub(super) fn create(args: &[String]) -> Result<Value> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    #[cfg(target_os = "linux")]
    let executable = PathBuf::from("/proc/self/exe");
    #[cfg(not(target_os = "linux"))]
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let running =
        crate::platform::image_identity().ok_or("Cannot verify the worktree wrapper image")?;
    if crate::platform::file_identity(&executable) != Some(running) {
        return Err("Worktree worker image changed".into());
    }
    let mut command = Command::new(executable);
    command
        .args(["__state", "worktree-request-worker", &image_proof(running)])
        .args(args)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = crate::platform::spawn_in_turn(&mut command).map_err(|e| e.to_string())?;
    let lifeline = child.stdin.take();
    let mut encoded = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("Worktree worker has no output")?
        .take(65537)
        .read_to_end(&mut encoded);
    drop(lifeline);
    let status = child.wait().map_err(|e| e.to_string())?;
    read.map_err(|e| e.to_string())?;
    if !status.success() || encoded.len() > 65536 {
        return Err("Worktree worker interrupted. Check the original receipt.".into());
    }
    serde_json::from_slice::<Result<Value>>(&encoded)
        .map_err(|_| "Invalid worktree worker result".to_string())?
}

#[cfg(unix)]
pub(super) fn worker(args: &[String]) -> Result<i32> {
    use std::io::Write;
    let proof = args.first().ok_or("Missing worktree worker image")?;
    if crate::platform::image_identity().map(image_proof).as_ref() != Some(proof) {
        return Err("Worktree worker image changed".into());
    }
    unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        if libc::fstat(0, &mut stat) != 0 || stat.st_mode & libc::S_IFMT != libc::S_IFIFO {
            return Err("Worktree worker requires a wrapper lifeline".into());
        }
        let flags = libc::fcntl(0, libc::F_GETFL);
        if flags < 0 || libc::fcntl(0, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err("Cannot watch the worktree wrapper".into());
        }
    }
    LIFELINE.with(|slot| slot.set(true));
    let result = create_direct(&args[1..]);
    let bytes = serde_json::to_vec(&result).map_err(|e| e.to_string())?;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|e| e.to_string())?;
    Ok(0)
}
