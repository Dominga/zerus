//! Per-query resource refusal, not a selector or authority grant. This is used
//! only by ordinary display observations and their bounded background follow-up.
use super::*;
use std::cell::RefCell;
use std::time::Instant;

pub(in crate::state) const ENTRY_LIMIT: usize = 256;

struct Budget {
    until: Instant,
    bytes: usize,
    entries: usize,
    failed: bool,
}
thread_local! {
    static BUDGET: RefCell<Option<Budget>> = const {RefCell::new(None)};
    static ROUTE: RefCell<Option<(String,PathBuf)>> = const {RefCell::new(None)};
    static PUBLIC: RefCell<Option<(String,String)>> = const {RefCell::new(None)};
}
/// One already-observed ordinary composer, only for display normalization.
/// Native question/admission helpers must not consume the compact record.
pub(in crate::state) struct Public(Option<(String, String)>);
impl Public {
    pub fn enter(record: &Value) -> Self {
        Self(PUBLIC.with(|slot| {
            slot.replace(Some((
                string(record, "name").into(),
                string(record, "run_id").into(),
            )))
        }))
    }
}
impl Drop for Public {
    fn drop(&mut self) {
        PUBLIC.with(|slot| slot.replace(self.0.take()));
    }
}
pub(in crate::state) fn public(record: &Value) -> bool {
    active()
        && PUBLIC.with(|slot| {
            slot.borrow().as_ref().is_some_and(|(name, run)| {
                name == string(record, "name") && run == string(record, "run_id")
            })
        })
}
pub(in crate::state) struct Scope(Option<Budget>);
impl Scope {
    pub fn enter(until: Instant) -> Self {
        Self(BUDGET.with(|budget| {
            budget.replace(Some(Budget {
                until,
                bytes: 16 << 20,
                entries: ENTRY_LIMIT,
                failed: false,
            }))
        }))
    }
    pub fn verify(&self) -> Result<()> {
        BUDGET.with(|budget| {
            if budget
                .borrow()
                .as_ref()
                .is_some_and(|budget| !budget.failed && Instant::now() < budget.until)
            {
                Ok(())
            } else {
                Err("Session listing canonical overlay is incomplete or exceeded its bound".into())
            }
        })
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        BUDGET.with(|budget| {
            budget.replace(self.0.take());
        });
    }
}
pub(in crate::state) struct Route(Option<(String, PathBuf)>);
impl Route {
    /// The caller has already read and verified this exact canonical file.
    pub fn verified(path: PathBuf, name: &str) -> Self {
        Self(ROUTE.with(|route| route.replace(Some((name.into(), path)))))
    }
}
impl Drop for Route {
    fn drop(&mut self) {
        ROUTE.with(|route| {
            route.replace(self.0.take());
        });
    }
}
pub(in crate::state) fn active() -> bool {
    BUDGET.with(|budget| budget.borrow().is_some())
}
pub(in crate::state) fn deadline() -> Option<Instant> {
    BUDGET.with(|budget| budget.borrow().as_ref().map(|budget| budget.until))
}
pub(in crate::state) fn refuse() -> String {
    BUDGET.with(|budget| {
        if let Some(budget) = budget.borrow_mut().as_mut() {
            budget.failed = true;
        }
    });
    "Session listing needs an exact bounded canonical observation; retry after background recovery"
        .into()
}
pub(in crate::state) fn route(name: &str) -> Option<PathBuf> {
    if !active() {
        return None;
    }
    if let Some(path) = ROUTE.with(|route| {
        route
            .borrow()
            .as_ref()
            .filter(|(selected, _)| selected == name)
            .map(|(_, path)| path.clone())
    }) {
        return Some(path);
    }
    let path = label_record_path(name);
    if !path.exists() {
        refuse();
    }
    Some(path)
}

pub(in crate::state) fn read(path: &Path) -> Option<io::Result<Vec<u8>>> {
    let (until, limit) = BUDGET.with(|budget| {
        budget
            .borrow()
            .as_ref()
            .map(|budget| (budget.until, budget.bytes))
    })?;
    let result = (|| -> io::Result<Vec<u8>> {
        use std::io::Read;
        if Instant::now() >= until {
            return Err(io::Error::other("Listing observation expired"));
        }
        let mut file = crate::platform::no_follow(OpenOptions::new().read(true)).open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > limit as u64 {
            return Err(io::Error::other("Listing canonical read budget exceeded"));
        }
        let mut bytes = Vec::new();
        let mut chunk = [0; 64 << 10];
        loop {
            if Instant::now() >= until {
                return Err(io::Error::other("Listing observation expired"));
            }
            let room = limit.saturating_sub(bytes.len());
            if room == 0 {
                if file.read(&mut [0u8; 1])? != 0 {
                    return Err(io::Error::other("Listing canonical read budget exceeded"));
                }
                break;
            }
            let n = file.read(&mut chunk[..room.min(64 << 10)])?;
            BUDGET.with(|budget| budget.borrow_mut().as_mut().unwrap().bytes -= n);
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        Ok(bytes)
    })();
    if result.is_err() {
        refuse();
    }
    Some(result)
}
pub(in crate::state) fn paths(directory: &Path) -> Option<Result<Vec<PathBuf>>> {
    if !active() {
        return None;
    }
    let run = || -> Result<Vec<PathBuf>> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let allowed = BUDGET.with(|budget| {
                let mut budget = budget.borrow_mut();
                let budget = budget.as_mut().unwrap();
                if budget.entries == 0 || Instant::now() >= budget.until {
                    false
                } else {
                    budget.entries -= 1;
                    true
                }
            });
            if !allowed {
                return Err(refuse());
            }
            let path = entry.map_err(|error| error.to_string())?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    };
    let result = run();
    if result.is_err() {
        refuse();
    }
    Some(result)
}

pub(in crate::state) fn check() -> Result<()> {
    BUDGET.with(|budget| match budget.borrow().as_ref() {
        Some(budget) if budget.failed || Instant::now() >= budget.until => {
            Err("Session listing canonical overlay is incomplete or exceeded its bound".into())
        }
        _ => Ok(()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_observation_bounds_implicit_inventory_reads_bytes_entries_and_time() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("record.json");
        let file = fs::File::create(&path).unwrap();
        file.set_len(9 << 20).unwrap();
        {
            let scope = Scope::enter(Instant::now() + std::time::Duration::from_secs(2));
            assert!(read(&path).unwrap().is_ok());
            assert!(read(&path).unwrap().is_err());
            assert!(scope.verify().is_err());
        }
        {
            let scope = Scope::enter(Instant::now() + std::time::Duration::from_secs(2));
            assert!(
                records().is_err(),
                "a display overlay performed a whole inventory read"
            );
            assert!(scope.verify().is_err());
        }
        for n in 0..257 {
            fs::write(directory.path().join(format!("{n}.json")), b"{}").unwrap();
        }
        {
            let scope = Scope::enter(Instant::now() + std::time::Duration::from_secs(2));
            assert!(paths(directory.path()).unwrap().is_err());
            assert!(scope.verify().is_err());
        }
        {
            let scope = Scope::enter(Instant::now());
            assert!(read(&path).unwrap().is_err());
            assert!(scope.verify().is_err());
        }
        assert!(
            read(&path).is_none(),
            "scope leaked to the ordinary canonical path"
        );
    }
    #[test]
    fn public_display_context_is_row_local_and_never_native_admission() {
        let row = json!({"name":"claude/example/public","run_id":"run","agent":"claude","phase":"working"});
        let scope = Scope::enter(Instant::now() + std::time::Duration::from_secs(1));
        assert!(!public(&row));
        {
            let _public = Public::enter(&row);
            assert!(public(&row));
            assert!(!public(
                &json!({"name":"claude/example/public","run_id":"another"})
            ));
            // An accidental direct native-card request refuses the entire body;
            // it cannot reach identity/channel/admission with a compact record.
            assert!(crate::state::claude_approval::current(&row).is_none());
            assert!(scope.verify().is_err());
        }
        assert!(!public(&row));
    }

    #[test]
    #[cfg(unix)]
    fn a_canonical_overlay_never_waits_for_a_fifo() {
        use std::os::unix::ffi::OsStrExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("pipe");
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let started = Instant::now();
        let _scope = Scope::enter(started + std::time::Duration::from_secs(1));
        assert!(read(&path).unwrap().is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }
}
