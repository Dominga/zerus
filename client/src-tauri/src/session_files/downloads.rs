//! Joined transfers, using the existing single DownloadCache worker. No cache
//! file or lock implementation lives here; hgs still owns the actual transfer.
use super::{lock, now_ms};
use crate::{composer::Spawn, files::Files};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use zerus_client_core::download_cache::{self, Source};

pub(super) type Answer = Result<(PathBuf, Option<String>), String>;
type Done = Box<dyn FnOnce(Answer) + Send>;
pub(super) type Transfer = Arc<dyn Fn(&str, &str, &Path) -> Result<(), String> + Send + Sync>;
type Key = (PathBuf, String, String);
struct Job {
    force: bool,
    waiters: Vec<Done>,
}
pub(super) struct Downloads {
    cache: Files,
    spawn: Spawn,
    transfer: Transfer,
    jobs: Mutex<HashMap<Key, Job>>,
}
impl Downloads {
    pub fn new(cache: Files, spawn: Spawn, transfer: Transfer) -> Arc<Self> {
        Arc::new(Self {
            cache,
            spawn,
            transfer,
            jobs: Mutex::new(HashMap::new()),
        })
    }
    pub fn cached(self: &Arc<Self>, root: PathBuf, source: Source, force: bool, done: Done) {
        let key = (root, source.host, source.path);
        {
            let mut jobs = lock(&self.jobs);
            if let Some(job) = jobs.get_mut(&key) {
                if job.waiters.len() >= 256 {
                    drop(jobs);
                    done(Err("The file transfer did not complete.".into()));
                    return;
                }
                job.force |= force;
                job.waiters.push(done);
                return;
            }
            if jobs.len() >= 32 {
                drop(jobs);
                done(Err("The file transfer did not complete.".into()));
                return;
            }
            jobs.insert(
                key.clone(),
                Job {
                    force,
                    waiters: vec![done],
                },
            );
        }
        let me = self.clone();
        (self.spawn)(Box::new(move || me.run_cached(key)));
    }
    fn run_cached(&self, key: Key) {
        let source = Source {
            host: key.1.clone(),
            path: key.2.clone(),
        };
        let lookup = {
            let jobs = lock(&self.jobs);
            !jobs.get(&key).is_none_or(|job| job.force)
        };
        if lookup {
            let (root, source) = (key.0.clone(), source.clone());
            let cached = self
                .cache
                .run(move || download_cache::lookup(&root, &source, now_ms()));
            match cached.and_then(|result| result) {
                Err(error) => {
                    self.finish(&key, Err(error));
                    return;
                }
                Ok(Some(path)) => {
                    // Check force and remove the job under the same lock. A
                    // refresh joining a slow hash cannot receive the old copy.
                    let ready = {
                        let mut jobs = lock(&self.jobs);
                        if jobs.get(&key).is_some_and(|job| !job.force) {
                            jobs.remove(&key)
                        } else {
                            None
                        }
                    };
                    if let Some(job) = ready {
                        for done in job.waiters {
                            done(Ok((path.clone(), None)));
                        }
                        return;
                    }
                }
                Ok(None) => {}
            }
        }
        let root = key.0.clone();
        let result = self
            .cache
            .run(move || download_cache::reserve(&root))
            .and_then(|result| result)
            .and_then(|part| {
                (self.transfer)(&source.host, &source.path, part.path())?;
                let name = download_cache::filename(&source.path);
                self.cache
                    .run(move || download_cache::complete(part, &name, Some(&source), now_ms()))?
                    .map(|complete| (complete.path, complete.warning))
            });
        self.finish(&key, result);
    }
    fn finish(&self, key: &Key, result: Answer) {
        let job = lock(&self.jobs).remove(key);
        if let Some(job) = job {
            for (index, done) in job.waiters.into_iter().enumerate() {
                let mut answer = result.clone();
                if index > 0 {
                    if let Ok((_, warning)) = &mut answer {
                        *warning = None;
                    }
                }
                done(answer);
            }
        }
    }
    pub fn direct(self: &Arc<Self>, source: Source, path: PathBuf, done: Done) {
        let me = self.clone();
        (self.spawn)(Box::new(move || {
            let result = (me.transfer)(&source.host, &source.path, &path).map(|()| (path, None));
            done(result);
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> std::io::Result<Self> {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "zerus-joined-download-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root)?;
            Ok(Self(root))
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn source() -> Source {
        Source {
            host: "example-peer".into(),
            path: "/srv/example/report.md".into(),
        }
    }
    fn receiver() -> (Done, mpsc::Receiver<Answer>) {
        let (done, answer) = mpsc::channel();
        (
            Box::new(move |result| {
                done.send(result).unwrap();
            }),
            answer,
        )
    }
    fn drain(work: &Mutex<Vec<crate::composer::Job>>) {
        let jobs = std::mem::take(&mut *lock(work));
        for job in jobs {
            job();
        }
    }
    #[test]
    fn joined_refresh_does_not_answer_an_earlier_cached_copy_and_new_root_is_distinct() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("configured-downloads");
        let part = download_cache::reserve(&root).unwrap();
        std::fs::write(part.path(), b"old").unwrap();
        let old = download_cache::complete(part, "report.md", Some(&source()), now_ms())
            .unwrap()
            .path;
        let transfers = Arc::new(AtomicUsize::new(0));
        let called = transfers.clone();
        let work = Arc::new(Mutex::new(Vec::new()));
        let queue = work.clone();
        let downloads = Downloads::new(
            crate::files::start(),
            Arc::new(move |job| lock(&queue).push(job)),
            Arc::new(move |host, path, output| {
                assert_eq!((host, path), ("example-peer", "/srv/example/report.md"));
                called.fetch_add(1, Ordering::SeqCst);
                std::fs::write(output, b"fresh").unwrap();
                Ok(())
            }),
        );
        let (first, a) = receiver();
        let (refresh, b) = receiver();
        downloads.cached(root.clone(), source(), false, first);
        downloads.cached(root.clone(), source(), true, refresh);
        assert_eq!(lock(&work).len(), 1);
        drain(&work);
        let fresh = a.recv().unwrap().unwrap().0;
        assert_eq!(fresh, b.recv().unwrap().unwrap().0);
        assert_ne!(fresh, old);
        assert_eq!(std::fs::read(&fresh).unwrap(), b"fresh");
        assert_eq!(transfers.load(Ordering::SeqCst), 1);
        let (same, c) = receiver();
        downloads.cached(root, source(), false, same);
        drain(&work);
        assert_eq!(c.recv().unwrap().unwrap().0, fresh);
        assert_eq!(transfers.load(Ordering::SeqCst), 1);
        let (other, d) = receiver();
        downloads.cached(temp.path().join("another-root"), source(), false, other);
        drain(&work);
        assert_ne!(d.recv().unwrap().unwrap().0, fresh);
        assert_eq!(transfers.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn failed_joined_transfers_answer_every_request_remove_the_part_and_allow_explicit_retry() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("cache");
        let work = Arc::new(Mutex::new(Vec::new()));
        let queue = work.clone();
        let transfers = Arc::new(AtomicUsize::new(0));
        let called = transfers.clone();
        let downloads = Downloads::new(
            crate::files::start(),
            Arc::new(move |job| lock(&queue).push(job)),
            Arc::new(move |_, _, output| {
                if called.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err("Synthetic transfer failure.".into());
                }
                std::fs::write(output, b"recovered").unwrap();
                Ok(())
            }),
        );
        let (one, a) = receiver();
        let (two, b) = receiver();
        downloads.cached(root.clone(), source(), false, one);
        downloads.cached(root.clone(), source(), true, two);
        drain(&work);
        assert_eq!(
            a.recv().unwrap().unwrap_err(),
            "Synthetic transfer failure."
        );
        assert_eq!(
            b.recv().unwrap().unwrap_err(),
            "Synthetic transfer failure."
        );
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".part")));
        let (retry, c) = receiver();
        downloads.cached(root, source(), false, retry);
        drain(&work);
        assert_eq!(
            std::fs::read(c.recv().unwrap().unwrap().0).unwrap(),
            b"recovered"
        );
        assert_eq!(transfers.load(Ordering::SeqCst), 2);
    }
}
