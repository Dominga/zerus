//! A search starts one private, finite catch-up owner. Its bounded slices resume
//! persisted queue/cursors; no session action or canonical write uses this cache.
use super::*;
use fs2::FileExt;

fn guard(state: &Path, name: &str) -> Result<Option<storage::Guard>> {
    let root = super::super::repositories::directory::Directory::open(state)?;
    let directory = root.ensure_child("content-search-v1")?;
    let file = directory.open_private_lock(name)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(storage::Guard::held(file))),
        Err(error) if crate::platform::lock_contended(&error) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
pub(in crate::state) fn request(_: &Path) {}
#[cfg(not(test))]
pub(in crate::state) fn request(state: &Path) {
    use std::process::{Command, Stdio};
    let _ = (|| -> Result<()> {
        let state = crate::platform::canonical(state).map_err(|e| e.to_string())?;
        let Some(_launch) = guard(&state, "launch.lock")? else {
            return Ok(());
        };
        let Some(probe) = guard(&state, "worker.lock")? else {
            return Ok(());
        };
        let attempt = state.join("content-search-v1/worker-attempt.json");
        let at = labels::read_file(&attempt, 4096)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
            .and_then(|value| value["at"].as_f64())
            .unwrap_or(0.);
        if (0.0..1.0).contains(&(now() - at)) {
            return Ok(());
        }
        atomic(&attempt, &json!({"at":now()}).to_string())?;
        let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        for (key, _) in
            std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("HGS_"))
        {
            command.env_remove(key);
        }
        command
            .args(["__state", "content-search-worker"])
            .env("HGS_STATE_DIR", &state)
            .current_dir(&state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        drop(probe);
        let mut child = crate::platform::spawn_detached(&mut command).map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    })();
}

pub(in crate::state) fn run() -> Result<i32> {
    lower_own_priority();
    let state = crate::platform::canonical(root()).map_err(|e| e.to_string())?;
    let original = super::super::repositories::directory::Directory::open(&state)?;
    let Some(_owner) = guard(&state, "worker.lock")? else {
        return Ok(0);
    };
    let directory = original.child("content-search-v1")?;
    let started = now();
    directory.replace_file(
        "worker.json",
        &serde_json::to_vec(&json!({"pid":std::process::id(),"started":started,"state":"running"}))
            .map_err(|e| e.to_string())?,
    )?;
    let result = workload(&state, &original);
    original.verify()?;
    let (phase, error) = match &result {
        Ok(true) => ("complete", String::new()),
        Ok(false) => ("pending", String::new()),
        Err(error) => ("unavailable", error.chars().take(512).collect()),
    };
    directory.replace_file("worker.json", &serde_json::to_vec(&json!({"pid":std::process::id(),"started":started,"finished":now(),"state":phase,"error":error})).map_err(|e|e.to_string())?)?;
    result.map(|_| 0)
}

fn workload(
    state: &Path,
    original: &super::super::repositories::directory::Directory,
) -> Result<bool> {
    let mut store = Store::open(&state.join("content-search-v1"))?;
    let cancel = AtomicBool::new(false);
    let until = Instant::now() + Duration::from_secs(120);
    for _ in 0..8192 {
        if Instant::now() >= until {
            return Ok(false);
        }
        original.verify()?;
        store.discover(
            state,
            &DiscoveryBudget {
                entries: 256,
                path_bytes: 1 << 20,
                deadline: Instant::now() + Duration::from_millis(25),
            },
            &cancel,
        )?;
        store.consume(state, &Budget::interactive(), &cancel)?;
        let status = store.catalog_status()?;
        if status["discovery"] == "complete"
            && status["pending_paths"] == 0
            && status["decoder_pending"] != true
        {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(false)
}

/// A small cold computer may finish preparation during its first search. Large
/// inventories leave immediately for bounded fallback; their helper continues.
pub(in crate::state) fn wait_small(state: &Path) {
    let until = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < until {
        if let Some(db) = metadata_cache::open(state) {
            let totals: Option<(i64, i64)> = db
                .query_row(
                    "SELECT paths,pending FROM catalog_totals WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            if let Some((paths, pending)) = totals {
                if paths > 500 {
                    return;
                }
                let complete: bool = db.query_row("SELECT COALESCE((SELECT value='1' FROM meta WHERE key='catalog_complete'),0)", [], |r|r.get(0)).unwrap_or(false);
                let outdated: bool = db
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM catalog_queue WHERE reader_version<? LIMIT 1)",
                        [metadata_cache::VERSION],
                        |r| r.get(0),
                    )
                    .unwrap_or(true);
                if complete && pending == 0 && !outdated {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
