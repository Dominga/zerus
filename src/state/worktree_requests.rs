//! Private, opt-in creation receipts. An interrupted reservation is never replayed.
use super::*;
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fs::File;
use std::sync::Arc;
mod store;

/// Opaque native configured-root custody. The service retains this from startup;
/// a later metadata request cannot reopen/adopt a replacement root by pathname.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) struct ReceiptRoot(Arc<super::repositories::directory::Directory>);
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl ReceiptRoot {
    pub(crate) fn capture(path: &Path) -> Result<Self> {
        let path = crate::platform::canonical(path).map_err(|e| e.to_string())?;
        let root = super::repositories::directory::Directory::open(&path)?;
        if !root.private()? {
            return Err("Worktree receipt root must be private".into());
        }
        Ok(Self(Arc::new(root)))
    }
    pub(crate) fn receipt(&self, id: &str, fingerprint: Option<&str>) -> Result<Option<Value>> {
        token(&["--request-id".into(), id.into()])?;
        let store = Arc::new(store::Store::acquire_root(
            self.0.clone(),
            false,
            fingerprint.is_some(),
        )?);
        match fingerprint {
            Some(fingerprint) => fence_store(store, id, fingerprint).map(Some),
            None => store.read(id),
        }
    }
}

thread_local! {
    static ORIGINAL: RefCell<Option<Arc<store::Lock>>> = const { RefCell::new(None) };
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
        ORIGINAL.with(|slot| -> Result<()> {
            let slot = slot.borrow();
            let original = slot
                .as_ref()
                .ok_or("Missing original worktree reservation")?;
            let mut value = original
                .store
                .read(id)?
                .ok_or("Missing worktree reservation")?;
            value["effects_started"] = json!(true);
            original.write(id, &value, true)
        })?;
        EFFECTS.with(|slot| slot.set(true));
    }
    Ok(())
}

pub(super) fn cancelled() -> bool {
    if ORIGINAL.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|original| original.verify().is_err())
    }) {
        return true;
    }
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

/// Legacy non-receipt clients use opaque string IDs (including mobile API v1).
/// Keep that original path intact. Every UUID spelling enters the common
/// exclusion parser, which rejects aliases instead of bypassing a fence.
pub(super) fn has_receipt_id(args: &[String]) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == "--request-id" && uuid::Uuid::parse_str(&pair[1]).is_ok())
}

#[cfg(all(test, target_os = "linux"))]
fn folder() -> PathBuf {
    root().join("worktree-requests")
}
fn read(id: &str) -> Result<Option<Value>> {
    store::Store::observe(&root())?.read(id)
}

/// The original receipt for the worktree actor's read-only recovery. A
/// missing or unreadable receipt never authorizes another request.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn original_receipt(id: &str) -> Result<Option<Value>> {
    token(&["--request-id".into(), id.into()])?;
    read(id)
}

fn snapshot(id: &str) -> Result<Value> {
    let Some(mut value) = read(id)? else {
        return Ok(json!({"schema":1,"request_id":id,"status":"not_found"}));
    };
    if value["status"] == "running" {
        // A free original lock proves the worker is gone, not that Git did nothing.
        let store = Arc::new(store::Store::observe(&root())?);
        if store.observe_lock(id).is_ok() {
            value["status"] = json!("unconfirmed");
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

/// Explicit Stop seals future execution of this UUID in the original CLI
/// receipt lane. It NEVER translates absence into a historical no-effects
/// claim, and it never retires an actor, worker, pipe or service charge.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub(crate) fn fence_in(root: &Path, id: &str, fingerprint: &str) -> Result<Value> {
    token(&["--request-id".into(), id.into()])?;
    if fingerprint.len() != 64
        || !fingerprint
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("Invalid original worktree fingerprint".into());
    }
    // An absent original store cannot be repaired into absence proof. A newly
    // installed outside-store anchor protects future calls only.
    let store = Arc::new(store::Store::open(root, false)?);
    fence_store(store, id, fingerprint)
}
fn fence_store(store: Arc<store::Store>, id: &str, fingerprint: &str) -> Result<Value> {
    let guard = match store.try_lock(id)? {
        Some(guard) => guard,
        None => {
            return match store.read(id)? {
                Some(value) if value["fingerprint"] == fingerprint => {
                    store.request_cancel(id)?;
                    Ok(value)
                }
                Some(_) => {
                    Err("Worktree request arguments do not match the original request".into())
                }
                None => Err("Worktree request is still running or being reserved".into()),
            };
        }
    };
    if let Some(mut value) = store.read(id)? {
        if value["fingerprint"] != fingerprint {
            return Err("Worktree request arguments do not match the original request".into());
        }
        if value["status"] == "running" {
            value["status"] = json!("unconfirmed");
        }
        guard.verify()?;
        return Ok(value); // Existing receipts are never overwritten by Stop.
    }
    let value = json!({"schema":1,"request_id":id,"fingerprint":fingerprint,
        "status":"fenced","future_starts_fenced":true,"past_outcome":"unknown",
        "detail":"Setup stopped. Further starts of this request are fenced. The earlier attempt's outcome is unknown; inspect the destination before creating a new session.",
        "finished_at":now()});
    guard.write(id, &value, false)?;
    Ok(value)
}
#[cfg(all(test, target_os = "linux"))]
fn receipt_in(root: &Path, id: &str) -> Result<Option<Value>> {
    token(&["--request-id".into(), id.into()])?;
    store::Store::observe(root)?.read(id)
}

pub(super) fn control(action: &str, args: &[String]) -> Result<Value> {
    let id = token(args)?;
    let fingerprint = args
        .windows(2)
        .filter(|v| v[0] == "--fingerprint")
        .map(|v| v[1].as_str())
        .collect::<Vec<_>>();
    if action == "cancel"
        && args.len() == 5
        && fingerprint.len() == 1
        && args.iter().any(|a| a == "--json")
    {
        return fence_in(&root(), &id, fingerprint[0]);
    }
    if args.len() != 3 || !args.iter().any(|a| a == "--json") {
        return Err(
            "usage: hgs worktrees status|cancel --request-id UUID [--fingerprint SHA256] --json"
                .into(),
        );
    }
    let value = snapshot(&id)?;
    if action == "cancel" && value["status"] == "running" {
        let store = store::Store::open(&root(), false)?;
        store.request_cancel(&id)?;
    }
    Ok(value)
}

/// Runs in-process here: no self-exec to guard.
#[cfg(not(unix))]
pub(super) fn create(args: &[String]) -> Result<Value> {
    create_direct(args)
}

pub(super) fn create_direct(args: &[String]) -> Result<Value> {
    create_receipt(args, None)
}

/// An owned (service) worktree request. The actor's Reserve check precedes
/// the original receipt; the same receipt store means it is never replayed.
#[cfg(unix)]
#[cfg_attr(not(all(test, target_os = "linux")), allow(dead_code))] // Wired by the worktree actor.
pub(crate) fn create_owned(
    args: &[String],
    control: &super::worktrees::Control<'_>,
) -> Result<Value> {
    // Git binds the pinned common directory through its own descriptor
    // (/proc/self/fd), which only Linux offers. Elsewhere the owned effect is
    // refused before its reservation, receipt or any effect.
    if !cfg!(target_os = "linux") {
        return Err("creation_worktree_unsupported".into());
    }
    (control.before)(super::worktrees::Step::Reserve)?;
    create_receipt(args, Some(control))
}

/// Defense at the common checkout seam: an ID-bearing effect must come from
/// the same live original exclusion and receipt. A future in-process caller
/// cannot bypass a permanent fence by skipping the public CLI dispatcher.
pub(super) fn original_current(args: &[String]) -> Result<()> {
    let id = token(args)?;
    let arguments: Vec<_> = args.iter().filter(|v| *v != "--receipt").cloned().collect();
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&arguments).map_err(|e| e.to_string())?)
    );
    ORIGINAL.with(|slot| {
        let slot = slot.borrow();
        let original = slot
            .as_ref()
            .ok_or("Missing original worktree receipt exclusion")?;
        original.verify()?;
        let value = original
            .store
            .read(&id)?
            .ok_or("Missing original worktree receipt")?;
        if value["status"] != "running" || value["fingerprint"] != fingerprint {
            return Err("Worktree request is no longer the original running attempt".into());
        }
        Ok(())
    })
}

fn create_receipt(
    args: &[String],
    control: Option<&super::worktrees::Control<'_>>,
) -> Result<Value> {
    let id = token(args)?;
    let args: Vec<String> = args.iter().filter(|a| *a != "--receipt").cloned().collect();
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&args).map_err(|e| e.to_string())?)
    );
    let store = Arc::new(store::Store::open(&root(), true)?);
    let guard = match store.lock(&id) {
        Ok(guard) => Arc::new(guard),
        Err(error) => {
            return match store.read(&id)? {
                Some(value) if value["fingerprint"] == fingerprint => Ok(value),
                Some(_) => {
                    Err("Worktree request arguments do not match the original request".into())
                }
                None => Err(error),
            }
        }
    };
    if let Some(mut value) = store.read(&id)? {
        guard.verify()?;
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
    guard.write(&id, &receipt, false)?;
    ORIGINAL.with(|slot| *slot.borrow_mut() = Some(guard.clone()));
    CANCEL.with(|slot| *slot.borrow_mut() = Some(store.cancel_path(&id)));
    EFFECTS.with(|slot| slot.set(false));
    let result = super::worktrees::create_with(&args, control);
    CANCEL.with(|slot| *slot.borrow_mut() = None);
    ORIGINAL.with(|slot| *slot.borrow_mut() = None);
    match result {
        Ok(value) => {
            // An owned creation returns its ownership evidence, verified under
            // its own custody; it starts no plain Git lookup here.
            if control.is_none() {
                receipt["creation_owner"] = value["common_dir"]
                    .as_str()
                    .zip(value["path"].as_str())
                    .and_then(|(common, path)| {
                        super::worktree_lifecycle::owned(Path::new(common), Path::new(path))
                    })
                    .unwrap_or(Value::Null);
            }
            for (key, value) in value.as_object().ok_or("Invalid worktree result")? {
                receipt[key] = value.clone();
            }
        }
        Err(error) => {
            // After an owned creation's effects started, a refusal or identity
            // failure leaves an uncertain checkout: never Created or Failed.
            let effects = EFFECTS.with(std::cell::Cell::get);
            receipt["status"] = json!(if control.is_some() && effects {
                "unconfirmed"
            } else {
                "failed"
            });
            receipt["detail"] = json!(error.chars().take(4096).collect::<String>());
        }
    }
    receipt["finished_at"] = json!(now());
    receipt["effects_started"] = json!(EFFECTS.with(std::cell::Cell::get));
    guard.write(&id, &receipt, true)?;
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
    crate::self_exec::ensure().map_err(|e| e.to_string())?;
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

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn test_image_refuses_the_worker_wrapper_before_self_exec() {
        crate::self_exec::private_probe("state::worktree_requests::tests::worker_wrapper_probe");
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "run by test_image_refuses_the_worker_wrapper_before_self_exec"]
    fn worker_wrapper_probe() {
        crate::self_exec::probe_child("state::worktree_requests::tests::worker_wrapper_probe");
        let id = "11111111-1111-4111-8111-111111111111";
        let args = ["--receipt", "--request-id", id, "--json"].map(String::from);
        assert_eq!(
            super::create(&args).unwrap_err(),
            crate::self_exec::TEST_IMAGE
        );
        assert!(
            !super::folder().exists(),
            "no request receipt store is touched"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn explicit_fences_share_the_original_receipt_lane_without_claiming_past_effects() {
        crate::self_exec::private_probe("state::worktree_requests::tests::fence_probe");
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "run by explicit_fences_share_the_original_receipt_lane_without_claiming_past_effects"]
    fn fence_probe() {
        use super::super::worktrees::{Control, Step};
        use super::*;
        use std::sync::atomic::AtomicBool;
        crate::self_exec::probe_child("state::worktree_requests::tests::fence_probe");
        let path = root();
        private_dir(&path).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let plan = zerus_client_core::desktop::creation_worktree::Plan {
            worktree_request_id: id.clone(),
            expected_machine: "22222222-2222-4222-8222-222222222222".into(),
            path: "/fixture/repo".into(),
            common_dir: "/fixture/repo/.git".into(),
            branch: "fixture".into(),
            destination: "/fixture/new".into(),
        };
        let fp = plan.request_fingerprint().unwrap();
        assert!(
            fence_in(&path, &id, &fp).is_err(),
            "Stop must not create a missing receipt store"
        );
        let store = Arc::new(store::Store::open(&path, true).unwrap());
        let fence = fence_in(&path, &id, &fp).unwrap();
        assert!(zerus_client_core::desktop::creation_worktree::fenced(
            &fence, &plan
        ));
        assert!(fence.get("effects_started").is_none());
        let original_bytes = std::fs::read(folder().join(format!("{id}.json"))).unwrap();
        assert_eq!(
            create_direct(&plan.arguments()).unwrap(),
            fence,
            "delayed direct creator returns the original fence before checkout"
        );
        assert_eq!(
            super::super::worktrees::create_with(&plan.arguments(), None).unwrap_err(),
            "Missing original worktree receipt exclusion",
            "an ID-bearing in-process effect cannot bypass the receipt lane"
        );
        let lock_file = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path.join("private-custody"))
            .unwrap();
        let custody = zerus_client_core::run::custody::InheritedLock::try_lock(lock_file).unwrap();
        let steps = std::sync::Mutex::new(Vec::new());
        let cancelled = AtomicBool::new(false);
        let before = |step| {
            steps.lock().unwrap().push(step);
            Ok(())
        };
        let control = Control {
            before: &before,
            cancelled: &cancelled,
            custody: &custody,
        };
        assert_eq!(create_owned(&plan.arguments(), &control).unwrap(), fence);
        assert_eq!(
            *steps.lock().unwrap(),
            [Step::Reserve],
            "only original native admission is checked; no checkout callback"
        );
        assert_eq!(
            std::fs::read(folder().join(format!("{id}.json"))).unwrap(),
            original_bytes
        );
        let names = || {
            std::fs::read_dir(&path)
                .unwrap()
                .map(|v| v.unwrap().file_name())
                .collect::<std::collections::BTreeSet<_>>()
        };
        let before = names();
        assert_eq!(
            receipt_in(&path, &id).unwrap(),
            Some(fence.clone()),
            "lost Stop reply recovers by read only"
        );
        assert_eq!(names(), before);
        assert_eq!(fence_in(&path, &id, &fp).unwrap(), fence);
        assert!(fence_in(&path, &id, &"0".repeat(64)).is_err());
        // An active original cannot be turned into a closed fence.
        let busy = uuid::Uuid::new_v4().to_string();
        let held = store.lock(&busy).unwrap();
        assert!(fence_in(&path, &busy, &fp).is_err());
        assert!(store.read(&busy).unwrap().is_none());
        drop(held);
        // Neither partial nor unconfirmed existing evidence is overwritten.
        let partial = uuid::Uuid::new_v4().to_string();
        let held = store.lock(&partial).unwrap();
        let value = json!({"schema":1,"request_id":partial,"status":"unconfirmed", "fingerprint":fp,"effects_started":true});
        held.write(&partial, &value, false).unwrap();
        drop(held);
        assert_eq!(fence_in(&path, &partial, &fp).unwrap(), value);
        // A failed publish does not produce a successful/fenced answer.
        let failed = uuid::Uuid::new_v4().to_string();
        super::super::storage::cut::arm(0);
        assert!(fence_in(&path, &failed, &fp).is_err());
        super::super::storage::cut::disarm();
        assert!(store.read(&failed).unwrap().is_none());
        let unsynced = uuid::Uuid::new_v4().to_string();
        super::super::repositories::directory::fail_publication_sync(Some(folder()));
        assert!(
            fence_in(&path, &unsynced, &fp).is_err(),
            "a publication sync error cannot return a closed fence"
        );
        super::super::repositories::directory::fail_publication_sync(None);
        // A possibly visible negative still cannot assert no historical effects.
        assert!(store
            .read(&unsynced)
            .unwrap()
            .unwrap()
            .get("effects_started")
            .is_none());
        // Irregular native data never blocks and cannot mint a closed fence.
        for leaf in ["lock", "json"] {
            let invalid = uuid::Uuid::new_v4().to_string();
            let path = folder().join(format!("{invalid}.{leaf}"));
            let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            let start = std::time::Instant::now();
            assert!(fence_in(&root(), &invalid, &fp).is_err());
            assert!(start.elapsed() < std::time::Duration::from_secs(1));
        }
        let alias = uuid::Uuid::new_v4().to_string();
        std::os::unix::fs::symlink(
            folder().join(format!("{id}.json")),
            folder().join(format!("{alias}.json")),
        )
        .unwrap();
        assert!(fence_in(&root(), &alias, &fp).is_err());
        let replaced = uuid::Uuid::new_v4().to_string();
        let held = store.lock(&replaced).unwrap();
        let value = json!({"schema":1,"request_id":replaced,"fingerprint":fp,"status":"running"});
        held.write(&replaced, &value, false).unwrap();
        std::fs::rename(
            folder().join(format!("{replaced}.json")),
            root().join("retained-original-receipt.json"),
        )
        .unwrap();
        zerus_client_core::fsx::write_private_durable(
            &folder().join(format!("{replaced}.json")),
            &serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(held.verify().is_err());
        assert!(held.write(&replaced, &value, true).is_err());
        drop(held);
        let replaced = uuid::Uuid::new_v4().to_string();
        let held = store.lock(&replaced).unwrap();
        std::fs::rename(
            folder().join(format!("{replaced}.lock")),
            root().join("retained-original-exclusion.lock"),
        )
        .unwrap();
        assert!(held.verify().is_err());
        assert!(held.write(&replaced, &json!({}), false).is_err());
        drop(held);
        // A replacement valid-empty namespace is NOT historical coverage and
        // cannot discard an already recorded future fence.
        std::fs::rename(folder(), path.join("retained-original-requests")).unwrap();
        private_dir(&folder()).unwrap();
        assert!(store.verify().is_err());
        assert!(fence_in(&path, &id, &fp).is_err());
        assert!(receipt_in(&path, &id).is_err());
        assert!(!folder().join(format!("{id}.json")).exists());
    }

    /// Runs inside the private probe, whose HOME/state are private: a real
    /// repository fixture, the original custody lock and recorded steps.
    #[cfg(target_os = "linux")]
    fn owned(
        before: impl Fn(super::super::worktrees::Step) -> super::Result<()>,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        super::Result<serde_json::Value>,
    ) {
        use std::sync::{atomic::AtomicBool, Arc};
        let root = std::path::PathBuf::from(std::env::var("TMPDIR").unwrap());
        let repository = root.join("repo");
        std::fs::create_dir(&repository).unwrap();
        let destination = root.join("repo-zerus-example");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.join("custody.lock"))
            .unwrap();
        let custody: Arc<_> =
            zerus_client_core::run::custody::InheritedLock::try_lock(file).unwrap();
        let cancelled = AtomicBool::new(false);
        let id = "22222222-2222-4222-8222-222222222222";
        let args = [
            "--path",
            repository.to_str().unwrap(),
            "--common-dir",
            repository.join(".git").to_str().unwrap(),
            "--branch",
            "zerus-example",
            "--base",
            "HEAD",
            "--destination",
            destination.to_str().unwrap(),
            "--request-id",
            id,
            "--json",
        ]
        .map(String::from);
        let result = super::create_owned(
            &args,
            &super::super::worktrees::Control {
                before: &before,
                cancelled: &cancelled,
                custody: &custody,
            },
        );
        (
            super::folder().join(format!("{id}.json")),
            destination,
            result,
        )
    }

    /// A real repository with a commit, optionally tracking a local remote.
    #[cfg(target_os = "linux")]
    fn repository(root: &std::path::Path, tracked: bool) -> std::path::PathBuf {
        let git = |dir: &std::path::Path, args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args([
                    "-c",
                    "user.name=fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
        };
        std::fs::create_dir_all(root).unwrap();
        git(root, &["init", "-q", "-b", "main", "repo"]);
        let repo = root.join("repo").canonicalize().unwrap();
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
        if tracked {
            git(root, &["init", "-q", "--bare", "origin.git"]);
            let origin = root.join("origin.git");
            git(
                &repo,
                &["remote", "add", "origin", origin.to_str().unwrap()],
            );
            git(&repo, &["push", "-q", "origin", "main"]);
            git(&repo, &["config", "branch.main.remote", "origin"]);
            git(&repo, &["config", "branch.main.merge", "refs/heads/main"]);
        }
        repo
    }

    /// One owned creation with real Git through the direct test seam.
    #[cfg(target_os = "linux")]
    fn owned_git_create(
        repo: &std::path::Path,
        id: &str,
        before: impl Fn(super::super::worktrees::Step) -> super::Result<()>,
    ) -> (std::path::PathBuf, serde_json::Value) {
        use std::sync::{atomic::AtomicBool, Arc};
        let destination = repo.with_file_name("repo-zerus-owned");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(repo.with_file_name(format!("{id}.custody")))
            .unwrap();
        let custody: Arc<_> =
            zerus_client_core::run::custody::InheritedLock::try_lock(file).unwrap();
        let cancelled = AtomicBool::new(false);
        let args = [
            "--path",
            repo.to_str().unwrap(),
            "--common-dir",
            repo.join(".git").to_str().unwrap(),
            "--branch",
            "zerus-owned",
            "--base",
            "HEAD",
            "--destination",
            destination.to_str().unwrap(),
            "--request-id",
            id,
            "--json",
        ]
        .map(String::from);
        super::super::repositories::OWNED_GIT_DIRECT.with(|direct| direct.set(true));
        let receipt = super::create_owned(
            &args,
            &super::super::worktrees::Control {
                before: &before,
                cancelled: &cancelled,
                custody: &custody,
            },
        )
        .unwrap();
        super::super::repositories::OWNED_GIT_DIRECT.with(|direct| direct.set(false));
        (destination, receipt)
    }

    #[cfg(target_os = "linux")]
    fn config(repo: &std::path::Path, key: &str) -> Option<String> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["config", "--get", key])
            .output()
            .unwrap();
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
    #[cfg(target_os = "linux")]
    fn branch_exists(repo: &std::path::Path) -> bool {
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "--verify", "--quiet", "refs/heads/zerus-owned"])
            .output()
            .unwrap()
            .status
            .success()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn owned_creation_with_real_git_checks_every_step_and_its_pinned_objects() {
        crate::self_exec::private_probe_with(
            "state::worktree_requests::tests::owned_real_git_probe",
            120,
            96,
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "run by owned_creation_with_real_git_checks_every_step_and_its_pinned_objects"]
    fn owned_real_git_probe() {
        crate::self_exec::probe_child("state::worktree_requests::tests::owned_real_git_probe");
        use super::super::worktrees::Step;
        let base = std::path::PathBuf::from(std::env::var("TMPDIR").unwrap());
        // Every scenario has its own original request UUID.
        let next = std::cell::Cell::new(0u32);
        let id = || {
            next.set(next.get() + 1);
            format!("{:08x}-1111-4111-8111-111111111111", next.get())
        };

        // Baseline: all six steps, the checkout, tracking and ownership.
        // The repository's post-checkout hook runs once in the checkout, as
        // `worktree add` runs it.
        let calls = std::sync::Mutex::new(Vec::new());
        let repo = repository(&base.join("baseline"), true);
        let hook = repo.join(".git/hooks/post-checkout");
        std::fs::write(&hook, "#!/bin/sh\necho \"$1 $2 $3\" >> post-checkout.log\n").unwrap();
        std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let (destination, receipt) = owned_git_create(&repo, &id(), |step| {
            calls.lock().unwrap().push(step);
            Ok(())
        });
        assert_eq!(receipt["status"], "created", "{receipt}");
        let head = receipt["head"].as_str().unwrap();
        assert_eq!(
            std::fs::read_to_string(destination.join("post-checkout.log")).unwrap(),
            format!("{} {head} 1\n", "0".repeat(head.len()))
        );
        assert_eq!(
            *calls.lock().unwrap(),
            [
                Step::Reserve,
                Step::Destination,
                Step::Checkout,
                Step::Verify,
                Step::Track,
                Step::Own
            ]
        );
        assert!(destination.join(".git").is_file() && branch_exists(&repo));
        assert_eq!(
            config(&repo, "branch.zerus-owned.remote").as_deref(),
            Some("origin")
        );
        assert!(receipt["creation_owner"]["id"].is_string(), "{receipt}");
        let metadata =
            std::path::PathBuf::from(receipt["creation_owner"]["metadata"].as_str().unwrap());
        assert!(metadata.join("zerus-owner.json").is_file());

        // A repository without an inherited upstream passes the same six
        // checked steps; Track writes nothing there.
        let calls = std::sync::Mutex::new(Vec::new());
        let repo = repository(&base.join("no-upstream"), false);
        let (destination, receipt) = owned_git_create(&repo, &id(), |step| {
            calls.lock().unwrap().push(step);
            Ok(())
        });
        assert_eq!(receipt["status"], "created", "{receipt}");
        assert_eq!(
            *calls.lock().unwrap(),
            [
                Step::Reserve,
                Step::Destination,
                Step::Checkout,
                Step::Verify,
                Step::Track,
                Step::Own
            ]
        );
        assert!(destination.join(".git").is_file() && branch_exists(&repo));
        assert!(config(&repo, "branch.zerus-owned.remote").is_none());
        assert!(receipt["upstream"].is_null(), "{receipt}");
        assert!(receipt["creation_owner"]["id"].is_string(), "{receipt}");

        // A refused Track or Own leaves its write undone and the result unconfirmed.
        for (name, refused) in [("track", Step::Track), ("own", Step::Own)] {
            let repo = repository(&base.join(name), true);
            let (destination, receipt) = owned_git_create(&repo, &id(), |step| {
                if step == refused {
                    Err("creation_lease_revoked".into())
                } else {
                    Ok(())
                }
            });
            assert_eq!(receipt["status"], "unconfirmed", "{name}: {receipt}");
            assert!(destination.join(".git").is_file(), "{name}: checkout kept");
            assert_eq!(
                config(&repo, "branch.zerus-owned.remote").is_some(),
                refused == Step::Own,
                "{name}: tracking written only after its own check"
            );
            assert!(
                receipt.get("creation_owner").is_none_or(|v| v.is_null()),
                "{name}"
            );
        }

        // A replaced destination or parent before Checkout: nothing is checked
        // out into the replacement, and the branch is never created.
        for (name, swap_parent) in [("dest-swap", false), ("parent-swap", true)] {
            let repo = repository(&base.join(name).join("inner"), false);
            let (destination, receipt) = owned_git_create(&repo, &id(), |step| {
                if step == Step::Checkout {
                    let victim = if swap_parent {
                        repo.parent().unwrap().to_path_buf()
                    } else {
                        repo.with_file_name("repo-zerus-owned")
                    };
                    let moved = victim.with_file_name("moved");
                    std::fs::rename(&victim, &moved).unwrap();
                    std::fs::create_dir(&victim).unwrap();
                    if swap_parent {
                        // Keep the repository reachable at its admitted path.
                        std::fs::rename(moved.join("repo"), victim.join("repo")).unwrap();
                    }
                }
                Ok(())
            });
            assert_eq!(receipt["status"], "unconfirmed", "{name}: {receipt}");
            assert!(
                !destination.join(".git").exists(),
                "{name}: replacement untouched"
            );
            assert!(!branch_exists(&repo), "{name}: no branch");
        }

        // A redirected common directory before Checkout or Track: the
        // foreign repository never receives the branch or tracking writes.
        for (name, at) in [
            ("common-checkout", Step::Checkout),
            ("common-track", Step::Track),
        ] {
            let repo = repository(&base.join(name), true);
            let foreign = repository(&base.join(name).join("foreign"), false);
            let (_, receipt) = owned_git_create(&repo, &id(), |step| {
                if step == at {
                    std::fs::rename(repo.join(".git"), repo.join(".git-admitted")).unwrap();
                    std::os::unix::fs::symlink(foreign.join(".git"), repo.join(".git")).unwrap();
                }
                Ok(())
            });
            assert_eq!(receipt["status"], "unconfirmed", "{name}: {receipt}");
            assert!(
                !branch_exists(&foreign),
                "{name}: foreign repository untouched"
            );
            assert!(
                config(&foreign, "branch.zerus-owned.remote").is_none(),
                "{name}"
            );
        }

        // A checkout replaced before Own is never reported as Created.
        let repo = repository(&base.join("own-swap"), true);
        let (destination, receipt) = owned_git_create(&repo, &id(), |step| {
            if step == Step::Own {
                let moved = destination_of(&repo).with_file_name("moved");
                std::fs::rename(destination_of(&repo), &moved).unwrap();
                std::fs::create_dir(destination_of(&repo)).unwrap();
            }
            Ok(())
        });
        assert_eq!(receipt["status"], "unconfirmed", "{receipt}");
        assert!(
            std::fs::read_dir(&destination).unwrap().next().is_none(),
            "replacement stays empty"
        );

        // The common directory replaced after the checkout's final identity
        // checks, before Git's own open: Git still writes only into the pinned
        // object it holds, and the foreign repository is untouched.
        let repo = repository(&base.join("common-late"), false);
        let foreign = repository(&base.join("common-late").join("foreign"), false);
        let (admitted, link, target) = (
            repo.join(".git-admitted"),
            repo.join(".git"),
            foreign.join(".git"),
        );
        super::super::worktrees::BEFORE_OWNED_CHECKOUT.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::rename(&link, &admitted).unwrap();
                std::os::unix::fs::symlink(&target, &link).unwrap();
            }))
        });
        let (_, receipt) = owned_git_create(&repo, &id(), |_| Ok(()));
        assert_eq!(receipt["status"], "unconfirmed", "{receipt}");
        assert!(!branch_exists(&foreign), "foreign repository untouched");
        assert!(
            !foreign.join(".git/worktrees").exists(),
            "no foreign worktree entry"
        );
        assert!(
            branch_in(&repo.join(".git-admitted")),
            "the branch is in the pinned repository"
        );

        // The tree checkout runs after `worktree add`, with GIT_DIR and
        // GIT_COMMON_DIR bound to the pinned entry and common directory. A
        // common directory or entry replaced in between receives nothing.
        for (name, swap_common) in [("tree-common", true), ("tree-entry", false)] {
            let repo = repository(&base.join(name), false);
            let foreign = repository(&base.join(name).join("foreign"), false);
            let (git, foreign_git) = (repo.join(".git"), foreign.join(".git"));
            super::super::worktrees::BEFORE_OWNED_TREE.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    if swap_common {
                        std::fs::rename(&git, git.with_file_name(".git-admitted")).unwrap();
                        // A prepared foreign entry at the same name.
                        std::fs::create_dir_all(foreign_git.join("worktrees/-")).unwrap();
                        std::os::unix::fs::symlink(&foreign_git, &git).unwrap();
                    } else {
                        let entry = git.join("worktrees/-");
                        std::fs::rename(&entry, git.join("worktrees/moved-entry")).unwrap();
                        std::fs::create_dir(&entry).unwrap();
                    }
                }))
            });
            let (destination, receipt) = owned_git_create(&repo, &id(), |_| Ok(()));
            assert_eq!(receipt["status"], "unconfirmed", "{name}: {receipt}");
            let (written, untouched) = if swap_common {
                (
                    repo.join(".git-admitted/worktrees/-"),
                    foreign.join(".git/worktrees/-"),
                )
            } else {
                (
                    repo.join(".git/worktrees/moved-entry"),
                    repo.join(".git/worktrees/-"),
                )
            };
            assert!(written.join("index").is_file(), "{name}: pinned entry");
            assert!(
                std::fs::read_dir(&untouched).unwrap().next().is_none(),
                "{name}: the replacement receives no index"
            );
            assert!(
                destination.join(".git").is_file(),
                "{name}: the checkout is kept"
            );
        }

        // The common directory replaced while Git runs the tree checkout,
        // after its setup: a smudge filter is the scheduling point. Git's
        // files ref backend would resolve refs through the real path it took
        // at startup, so a ref update here would land in the foreign
        // repository, which has a branch of the same name. The checkout
        // updates no ref, and the foreign repository receives nothing.
        let root = base.join("tree-during");
        let repo = repository(&root, false);
        let foreign = repository(&root.join("foreign"), false);
        let fixture = |dir: &std::path::Path, args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args([
                    "-c",
                    "user.name=fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
        };
        fixture(
            &foreign,
            &["commit", "-q", "--allow-empty", "-m", "foreign"],
        );
        fixture(&foreign, &["branch", "zerus-owned"]);
        std::fs::write(repo.join(".gitattributes"), "* filter=swap\n").unwrap();
        std::fs::write(repo.join("file"), "content\n").unwrap();
        fixture(&repo, &["add", ".gitattributes", "file"]);
        fixture(&repo, &["commit", "-q", "-m", "filtered"]);
        let (mark, script) = (root.join("swapped"), root.join("swap.sh"));
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ ! -e '{mark}' ]; then : > '{mark}'; mv '{git}' '{git}-admitted' && ln -s '{foreign}' '{git}'; fi\nexec cat\n",
                mark = mark.display(),
                git = repo.join(".git").display(),
                foreign = foreign.join(".git").display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        fixture(
            &repo,
            &["config", "filter.swap.smudge", script.to_str().unwrap()],
        );
        let tree = |dir: &std::path::Path| {
            let mut found = std::collections::BTreeMap::new();
            let mut pending = vec![dir.to_path_buf()];
            while let Some(next) = pending.pop() {
                for item in std::fs::read_dir(&next).unwrap() {
                    let path = item.unwrap().path();
                    let kind = std::fs::symlink_metadata(&path).unwrap();
                    let content = if kind.is_dir() {
                        pending.push(path.clone());
                        Vec::new()
                    } else if kind.is_symlink() {
                        std::fs::read_link(&path)
                            .unwrap()
                            .into_os_string()
                            .into_encoded_bytes()
                    } else {
                        std::fs::read(&path).unwrap()
                    };
                    found.insert(path, content);
                }
            }
            found
        };
        let before = tree(&foreign.join(".git"));
        let (destination, receipt) = owned_git_create(&repo, &id(), |_| Ok(()));
        assert!(mark.exists(), "the swap ran inside the tree checkout");
        assert_eq!(receipt["status"], "unconfirmed", "{receipt}");
        assert_eq!(
            tree(&foreign.join(".git")),
            before,
            "the foreign repository receives no ref, reflog or lock"
        );
        assert!(
            repo.join(".git-admitted/worktrees/-/index").is_file(),
            "the index is in the pinned entry"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("file")).unwrap(),
            "content\n",
            "the work tree was checked out"
        );

        // Checkout pins the metadata entry it just made; Own writes only
        // through that object. A replacement at the same path (even a full
        // copy of Git's links), a changed backlink or an existing owner file
        // leaves the result unconfirmed, and no owner file is ever overwritten.
        for (name, swap, sentinel, backlink) in [
            ("own-replaced", true, true, false),
            ("own-replaced-copy", true, false, false),
            ("own-existing-marker", false, true, false),
            ("own-backlink-changed", false, false, true),
        ] {
            let repo = repository(&base.join(name), false);
            let entry = repo.join(".git/worktrees/-");
            let moved = entry.with_file_name("moved-entry");
            let (_, receipt) = owned_git_create(&repo, &id(), |step| {
                if step == Step::Own {
                    if swap {
                        std::fs::rename(&entry, &moved).unwrap();
                        std::fs::create_dir(&entry).unwrap();
                        // Only identity tells this copy from Git's entry.
                        for file in ["gitdir", "HEAD", "commondir", "index"] {
                            if moved.join(file).exists() {
                                std::fs::copy(moved.join(file), entry.join(file)).unwrap();
                            }
                        }
                    }
                    if sentinel {
                        std::fs::write(entry.join("zerus-owner.json"), "sentinel\n").unwrap();
                    }
                    if backlink {
                        std::fs::write(entry.join("gitdir"), "/elsewhere/.git\n").unwrap();
                    }
                }
                Ok(())
            });
            assert_eq!(receipt["status"], "unconfirmed", "{name}: {receipt}");
            let owner = std::fs::read_to_string(entry.join("zerus-owner.json")).ok();
            assert_eq!(
                owner.as_deref(),
                sentinel.then_some("sentinel\n"),
                "{name}: never written or overwritten"
            );
            assert!(
                !moved.join("zerus-owner.json").exists(),
                "{name}: no owner file in the moved entry"
            );
            assert!(
                receipt.get("creation_owner").is_none_or(|v| v.is_null()),
                "{name}"
            );
        }
    }
    #[cfg(target_os = "linux")]
    fn branch_in(git_dir: &std::path::Path) -> bool {
        std::process::Command::new("git")
            .arg("--git-dir")
            .arg(git_dir)
            .args(["rev-parse", "--verify", "--quiet", "refs/heads/zerus-owned"])
            .output()
            .unwrap()
            .status
            .success()
    }
    #[cfg(target_os = "linux")]
    fn destination_of(repo: &std::path::Path) -> std::path::PathBuf {
        repo.with_file_name("repo-zerus-owned")
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_anchored_checkout_follows_the_pinned_destination_not_its_path() {
        crate::self_exec::private_probe_with(
            "state::worktree_requests::tests::anchored_checkout_probe",
            120,
            96,
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "run by an_anchored_checkout_follows_the_pinned_destination_not_its_path"]
    fn anchored_checkout_probe() {
        crate::self_exec::probe_child("state::worktree_requests::tests::anchored_checkout_probe");
        use super::super::repositories::directory::Directory;
        let base = std::path::PathBuf::from(std::env::var("TMPDIR").unwrap()).join("anchored");
        let repo = repository(&base, false);
        let foreign = repository(&base.join("foreign"), false);
        let parent = Directory::open(&base.canonicalize().unwrap()).unwrap();
        let pinned = parent
            .create("checkout", ".zerus-worktree-fixture")
            .unwrap();
        let common = Directory::open(&repo.join(".git").canonicalize().unwrap()).unwrap();
        // The admitted repository's own commit, read before any swap.
        let head = String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        // Both admitted objects move; other directories take their paths.
        std::fs::rename(base.join("checkout"), base.join("moved")).unwrap();
        std::fs::create_dir(base.join("checkout")).unwrap();
        std::fs::rename(repo.join(".git"), repo.join(".git-moved")).unwrap();
        std::os::unix::fs::symlink(foreign.join(".git"), repo.join(".git")).unwrap();
        let git_dir = format!("--git-dir=/proc/self/fd/{}", common.descriptor());
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        super::super::repositories::OWNED_GIT_DIRECT.with(|direct| direct.set(true));
        // Without the identity checks (which would refuse first), Git's own
        // writes still land in both pinned objects, never at the replaced paths.
        let result = super::super::repositories::owned_git_unverified_for_test(
            &pinned,
            &[&common],
            Default::default(),
            &[
                git_dir.as_str(),
                "worktree",
                "add",
                "--quiet",
                "-b",
                "zerus-anchor",
                "--",
                ".",
                head.trim(),
            ],
            &cancelled,
        );
        super::super::repositories::OWNED_GIT_DIRECT.with(|direct| direct.set(false));
        assert!(
            base.join("moved/.git").is_file(),
            "checkout in the pinned object: {result:?}"
        );
        assert!(std::fs::read_dir(base.join("checkout"))
            .unwrap()
            .next()
            .is_none());
        let branch = |git_dir: &std::path::Path| {
            std::process::Command::new("git")
                .arg("--git-dir")
                .arg(git_dir)
                .args([
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    "refs/heads/zerus-anchor",
                ])
                .output()
                .unwrap()
                .status
                .success()
        };
        assert!(
            branch(&repo.join(".git-moved")),
            "branch in the pinned repository"
        );
        assert!(
            !branch(&foreign.join(".git")),
            "foreign repository untouched"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn owned_creation_checks_the_actor_before_its_receipt_and_never_runs_a_test_image() {
        crate::self_exec::private_probe("state::worktree_requests::tests::owned_creation_probe");
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "run by owned_creation_checks_the_actor_before_its_receipt_and_never_runs_a_test_image"]
    fn owned_creation_probe() {
        crate::self_exec::probe_child("state::worktree_requests::tests::owned_creation_probe");
        use super::super::worktrees::Step;
        // A refused Reserve leaves no receipt, store or destination.
        let calls = std::sync::Mutex::new(Vec::new());
        let (receipt, destination, result) = owned(|step| {
            calls.lock().unwrap().push(step);
            Err("creation_lease_revoked".into())
        });
        assert_eq!(result.unwrap_err(), "creation_lease_revoked");
        assert_eq!(*calls.lock().unwrap(), [Step::Reserve]);
        assert!(!super::folder().exists());
        assert!(!destination.exists() && !receipt.exists());
        std::fs::remove_dir(std::env::var("TMPDIR").unwrap() + "/repo").unwrap();
        std::fs::remove_file(std::env::var("TMPDIR").unwrap() + "/custody.lock").unwrap();
        // An admitted request writes its original receipt, then the Git worker
        // refuses this unit image before any destination or checkout effect.
        let calls = std::sync::Mutex::new(Vec::new());
        let (receipt, destination, result) = owned(|step| {
            calls.lock().unwrap().push(step);
            Ok(())
        });
        let value = result.unwrap();
        assert_eq!(value["status"], "failed", "{value}");
        assert_eq!(value["detail"], crate::self_exec::TEST_IMAGE);
        assert_eq!(value["effects_started"], false);
        assert_eq!(*calls.lock().unwrap(), [Step::Reserve]);
        assert!(!destination.exists());
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(saved, value);
        assert_eq!(Step::Own.ordinal(), 5);
    }
}
