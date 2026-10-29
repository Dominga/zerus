//! Activity attachments read into the private cache, and opened (Qt's
//! `AttachmentLoader`, `AttachmentViewer.h:162-243`): one read per file
//! however many views ask, at most three reads at once with an Open ahead of
//! thumbnails, and the last files read answered from their copies while the
//! copies last. Bytes never cross the IPC: a view shows a copy through the
//! `zerus-file` scheme by the path a read answers.
//!
//! Opening follows Qt's `ExternalFile`: an Activity attachment is another
//! machine's file (marked as downloaded, `AttachmentLoader::open`), a
//! composer's attachment this computer's (`MessageComposer::openAttachment`),
//! and a type that runs programs is shown in the file manager instead.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

use serde::Serialize;
use zerus_client_core::attachments;
use zerus_client_core::external_file::{self, Origin, Outcome};
use zerus_client_core::reads::{AttachmentRef, CachedAttachment};
use zerus_client_core::ComputerId;

use super::files::Cache;
use super::{lock, Composers, Spawn};

/// Reads one attachment into the cache root it is given (`hgs attachment`).
pub type FetchFn = Arc<
    dyn Fn(&ComputerId, &AttachmentRef, &Path) -> Result<CachedAttachment, String> + Send + Sync,
>;

/// A system call on a file: open it with its default application, or show
/// it in the file manager. True when it started.
pub type PathFn = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// The system's open and reveal.
#[derive(Clone)]
pub struct Opener {
    pub open: PathFn,
    pub reveal: PathFn,
}

/// An attachment in the cache, as a view shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Fetched {
    /// `cache/{digest}/{name}`, the copy's `zerus-file` path.
    pub path: String,
    pub name: String,
    pub mime: String,
    pub bytes: u64,
}

/// Reads at once (`AttachmentLoader::pump`).
const RUNNING: usize = 3;
/// Files read whose copies answer again without a read (`m_paths`).
const REMEMBERED: usize = 256;

/// `AttachmentLoader::fetch` for a file its message saved no reference to.
pub const NO_REFERENCE: &str = "This attachment has no saved file reference.";
/// `AttachmentLoader::open` when no application opened the file.
pub const OPEN_FAILED: &str = "No application could open this file.";
/// `MessageComposer::openAttachment` when the copy could not be made.
pub const DRAFT_UNAVAILABLE: &str = "Could not open the attachment.";
/// `MessageComposer::openAttachment` when no application opened the file.
pub const DRAFT_OPEN_FAILED: &str = "Could not open the attachment with its default application.";

type Answer = Result<(Fetched, PathBuf), String>;

struct Job {
    computer: ComputerId,
    file: AttachmentRef,
    waiters: Vec<mpsc::Sender<Answer>>,
}

#[derive(Default)]
struct Queue {
    jobs: HashMap<String, Job>,
    /// Jobs not started yet, the next first.
    waiting: VecDeque<String>,
    running: usize,
    /// Files read, oldest first, with their copies.
    read: HashMap<String, (Fetched, PathBuf)>,
    order: VecDeque<String>,
}

impl Queue {
    /// The jobs to start now, keeping at most `RUNNING` reads.
    fn next(&mut self) -> Vec<(String, ComputerId, AttachmentRef)> {
        let mut start = Vec::new();
        while self.running < RUNNING {
            let Some(key) = self.waiting.pop_front() else {
                break;
            };
            if let Some(job) = self.jobs.get(&key) {
                self.running += 1;
                start.push((key, job.computer.clone(), job.file.clone()));
            }
        }
        start
    }

    fn remember(&mut self, key: String, found: (Fetched, PathBuf)) {
        if self.read.insert(key.clone(), found).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > REMEMBERED {
            if let Some(oldest) = self.order.pop_front() {
                self.read.remove(&oldest);
            }
        }
    }
}

/// Reads Activity attachments into the cache.
pub struct Fetcher {
    cache: Arc<Cache>,
    fetch: FetchFn,
    spawn: Spawn,
    queue: Mutex<Queue>,
}

/// One file of one message of one conversation on one computer
/// (`AttachmentLoader::fetch`'s key).
fn key_of(computer: &str, file: &AttachmentRef) -> String {
    serde_json::json!([
        computer,
        file.name,
        file.conversation_id,
        file.archive_id,
        file.agent_id,
        file.request_id,
        file.index
    ])
    .to_string()
}

impl Fetcher {
    pub fn new(cache: Arc<Cache>, fetch: FetchFn, spawn: Spawn) -> Arc<Self> {
        Arc::new(Fetcher {
            cache,
            fetch,
            spawn,
            queue: Mutex::new(Queue::default()),
        })
    }

    pub fn cache(&self) -> &Arc<Cache> {
        &self.cache
    }

    /// Blocks until the file is in the cache. A caller for a file being read
    /// waits for that read; `priority` moves it ahead of the reads waiting.
    pub fn fetch(
        self: &Arc<Self>,
        computer: &str,
        file: AttachmentRef,
        priority: bool,
    ) -> Result<Fetched, String> {
        self.local(computer, file, priority)
            .map(|(fetched, _)| fetched)
    }

    fn local(self: &Arc<Self>, computer: &str, file: AttachmentRef, priority: bool) -> Answer {
        if file.request_id.is_empty() {
            return Err(NO_REFERENCE.to_owned());
        }
        let key = key_of(computer, &file);
        // A file read before answers from its copy while the copy lasts.
        let known = lock(&self.queue).read.get(&key).cloned();
        if let Some((fetched, path)) = known {
            if attachments::cached_at(self.cache.root(), &path).is_some() {
                attachments::touch(&path);
                return Ok((fetched, path));
            }
        }
        let (sender, receiver) = mpsc::channel();
        let start = {
            let mut queue = lock(&self.queue);
            if let Some(job) = queue.jobs.get_mut(&key) {
                job.waiters.push(sender);
                let waiting = queue.waiting.iter().position(|k| *k == key);
                if let (true, Some(at)) = (priority, waiting) {
                    queue.waiting.remove(at);
                    queue.waiting.push_front(key);
                }
            } else {
                queue.jobs.insert(
                    key.clone(),
                    Job {
                        computer: ComputerId(computer.to_owned()),
                        file,
                        waiters: vec![sender],
                    },
                );
                if priority {
                    queue.waiting.push_front(key);
                } else {
                    queue.waiting.push_back(key);
                }
            }
            queue.next()
        };
        self.start(start);
        receiver
            .recv()
            .map_err(|_| "Could not read the attachment.".to_owned())?
    }

    fn start(self: &Arc<Self>, jobs: Vec<(String, ComputerId, AttachmentRef)>) {
        for (key, computer, file) in jobs {
            let me = Arc::clone(self);
            (self.spawn)(Box::new(move || me.run(key, computer, file)));
        }
    }

    fn run(self: &Arc<Self>, key: String, computer: ComputerId, file: AttachmentRef) {
        let result = (self.fetch)(&computer, &file, self.cache.root())
            .and_then(|cached| self.described(cached));
        if result.is_ok() {
            self.cache.prune_later(&self.spawn, Arc::new(HashSet::new));
        }
        let (waiters, next) = {
            let mut queue = lock(&self.queue);
            queue.running -= 1;
            let waiters = queue
                .jobs
                .remove(&key)
                .map(|job| job.waiters)
                .unwrap_or_default();
            if let Ok(found) = &result {
                queue.remember(key, found.clone());
            }
            (waiters, queue.next())
        };
        for waiter in waiters {
            let _ = waiter.send(result.clone());
        }
        self.start(next);
    }

    /// The view's description of a copy the read stored.
    fn described(&self, cached: CachedAttachment) -> Answer {
        let path = attachments::cached_at(self.cache.root(), &cached.local_path)
            .ok_or_else(|| "Could not save the attachment.".to_owned())?;
        let relative = path
            .strip_prefix(self.cache.root())
            .map_err(|_| "Could not save the attachment.".to_owned())?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
        let mime = match cached.metadata.get("mime").and_then(|v| v.as_str()) {
            Some(mime) if !mime.is_empty() => mime.to_owned(),
            _ => {
                let mut head = Vec::new();
                if let Ok(copy) = std::fs::File::open(&path) {
                    let _ =
                        std::io::Read::read_to_end(&mut std::io::Read::take(copy, 512), &mut head);
                }
                attachments::sniff(&head)
            }
        };
        let name = match cached.metadata.get("name").and_then(|v| v.as_str()) {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => "attachment".to_owned(),
        };
        Ok((
            Fetched {
                path: format!("cache/{relative}"),
                name,
                mime,
                bytes,
            },
            path,
        ))
    }

    /// Opens an Activity attachment (`AttachmentLoader::open`): a read ahead
    /// of the others, then the copy as another machine's file.
    pub fn open(
        self: &Arc<Self>,
        computer: &str,
        file: AttachmentRef,
        opener: &Opener,
    ) -> Result<Option<String>, String> {
        let (_, path) = self.local(computer, file, true)?;
        told(
            opened(&path, Origin::OtherMachine, opener),
            &path,
            OPEN_FAILED,
        )
    }
}

fn opened(path: &Path, origin: Origin, opener: &Opener) -> Outcome {
    external_file::open(path, origin, |p| (opener.open)(p), |p| (opener.reveal)(p))
}

/// What an open tells the person: nothing when it opened, a notice when the
/// file manager shows a program type, otherwise why it did not open.
fn told(outcome: Outcome, path: &Path, failed: &str) -> Result<Option<String>, String> {
    match outcome {
        Outcome::Opened => Ok(None),
        Outcome::Revealed => Ok(Some(external_file::revealed_notice(path))),
        Outcome::RevealFailed => Err(external_file::reveal_failed_notice(path)),
        Outcome::OpenFailed => Err(failed.to_owned()),
    }
}

/// Opens a composer's attachment (`MessageComposer::openAttachment`) by
/// the view's `draft/{handle}/{list}/{index}`: its copy in the cache, as
/// this computer's file. Nothing happens while the draft sends, or when its
/// attachment list is no longer the one the view showed.
pub fn open_draft(
    composers: &Composers,
    path: &str,
    opener: &Opener,
) -> Result<Option<String>, String> {
    let numbers = path
        .strip_prefix("draft/")
        .and_then(super::files::draft_numbers);
    let attachment = numbers.and_then(|[handle, list, index]| {
        let hub = composers.lock();
        hub.composers
            .values()
            .find(|c| c.handle == handle && c.attachments_rev == list && !c.sending)
            .and_then(|c| {
                c.draft
                    .attachments
                    .get(usize::try_from(index).ok()?)
                    .cloned()
            })
    });
    let Some(attachment) = attachment else {
        return Ok(None);
    };
    let cache = composers.cache();
    let files = attachments::prepare(cache.root(), std::slice::from_ref(&attachment))?;
    let path = files
        .into_iter()
        .next()
        .map(|file| file.local_path)
        .ok_or_else(|| DRAFT_UNAVAILABLE.to_owned())?;
    cache.prune_later(&composers.0.deps.spawn, Arc::new(HashSet::new));
    told(
        opened(&path, Origin::ThisComputer, opener),
        &path,
        DRAFT_OPEN_FAILED,
    )
}

/// Reads an Activity attachment into the cache for a view, after the reads
/// ahead of it.
#[tauri::command]
pub async fn attachment_fetch(
    fetcher: tauri::State<'_, Arc<Fetcher>>,
    computer: String,
    file: AttachmentRef,
) -> Result<Fetched, String> {
    let fetcher = Arc::clone(fetcher.inner());
    tauri::async_runtime::spawn_blocking(move || fetcher.fetch(&computer, file, false))
        .await
        .map_err(|error| error.to_string())?
}

/// A view shows the copy at `path` (`cache/{digest}/{name}`).
#[tauri::command]
pub fn attachment_pin(fetcher: tauri::State<'_, Arc<Fetcher>>, path: String) {
    if let Some(copy) = fetcher.cache().shown(&path) {
        fetcher.cache().pin(&copy);
    }
}

/// A view no longer shows the copy at `path`.
#[tauri::command]
pub fn attachment_unpin(fetcher: tauri::State<'_, Arc<Fetcher>>, path: String) {
    if let Some(copy) = fetcher.cache().shown_at(&path) {
        fetcher.cache().unpin(&copy);
    }
}

/// Opens an Activity attachment: `Ok(None)` when it opened, `Ok(notice)`
/// when the file manager shows it instead, otherwise why not.
#[tauri::command]
pub async fn attachment_open(
    fetcher: tauri::State<'_, Arc<Fetcher>>,
    opener: tauri::State<'_, Opener>,
    computer: String,
    file: AttachmentRef,
) -> Result<Option<String>, String> {
    let (fetcher, opener) = (Arc::clone(fetcher.inner()), opener.inner().clone());
    tauri::async_runtime::spawn_blocking(move || fetcher.open(&computer, file, &opener))
        .await
        .map_err(|error| error.to_string())?
}

/// Opens the composer attachment the view lists at `path`
/// (`draft/{handle}/{list}/{index}`), as `attachment_open` answers.
#[tauri::command]
pub async fn attachment_open_draft(
    composers: tauri::State<'_, Composers>,
    opener: tauri::State<'_, Opener>,
    path: String,
) -> Result<Option<String>, String> {
    let (composers, opener) = (composers.inner().clone(), opener.inner().clone());
    tauri::async_runtime::spawn_blocking(move || open_draft(&composers, &path, &opener))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::tests::{dir, hub, session, until};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Condvar;
    use std::time::{Duration, Instant};
    use zerus_client_core::attachments::Selection;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

    fn file(request: &str) -> AttachmentRef {
        AttachmentRef {
            name: "claude/sample-project/review".into(),
            request_id: request.into(),
            index: 0,
            conversation_id: "conv-1".into(),
            archive_id: None,
            agent_id: None,
        }
    }

    fn threads() -> Spawn {
        Arc::new(|job| {
            std::thread::spawn(job);
        })
    }

    /// A read that stores `name` with `bytes`, counting its calls.
    fn storing(name: &'static str, bytes: &'static [u8], calls: &Arc<AtomicUsize>) -> FetchFn {
        let calls = Arc::clone(calls);
        Arc::new(move |_, file, root| {
            calls.fetch_add(1, Ordering::SeqCst);
            let local_path = attachments::store_at(root, name, bytes).unwrap();
            Ok(CachedAttachment {
                metadata: serde_json::json!({"name": name, "request_id": file.request_id}),
                local_path,
            })
        })
    }

    fn wait_for(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_same_attachment_is_read_once_for_two_views() {
        let root = dir("fetch-once");
        let calls = Arc::new(AtomicUsize::new(0));
        let slow = storing("shot.png", PNG, &calls);
        let fetch: FetchFn = Arc::new(move |computer, file, root| {
            std::thread::sleep(Duration::from_millis(100));
            slow(computer, file, root)
        });
        let fetcher = Fetcher::new(Cache::new(root.join("cache")), fetch, threads());
        let (a, b) = (Arc::clone(&fetcher), Arc::clone(&fetcher));
        let one = std::thread::spawn(move || a.fetch("studio", file("req-1"), false));
        let two = std::thread::spawn(move || b.fetch("studio", file("req-1"), true));
        let (one, two) = (one.join().unwrap().unwrap(), two.join().unwrap().unwrap());
        assert_eq!(one, two);
        assert!(one.path.starts_with("cache/") && one.path.ends_with("/shot.png"));
        assert_eq!(
            (one.name.as_str(), one.mime.as_str(), one.bytes),
            ("shot.png", "image/png", PNG.len() as u64)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            fetcher.cache().shown(&one.path).is_some(),
            "the path names the copy"
        );
    }

    #[test]
    fn three_reads_run_at_once_and_an_open_goes_next() {
        let root = dir("fetch-queue");
        // Each read waits for a permit; `started` lists the reads in order.
        let gate = Arc::new((Mutex::new(0usize), Condvar::new()));
        let started = Arc::new(Mutex::new(Vec::<String>::new()));
        let (running, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let fetch: FetchFn = {
            let (gate, started, running, most) = (
                Arc::clone(&gate),
                Arc::clone(&started),
                Arc::clone(&running),
                Arc::clone(&most),
            );
            Arc::new(move |_, file, root| {
                started.lock().unwrap().push(file.request_id.clone());
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                let (permits, ready) = &*gate;
                let mut permits = ready
                    .wait_while(permits.lock().unwrap(), |n| *n == 0)
                    .unwrap();
                *permits -= 1;
                drop(permits);
                running.fetch_sub(1, Ordering::SeqCst);
                let local_path = attachments::store_at(
                    root,
                    &format!("{}.txt", file.request_id),
                    file.request_id.as_bytes(),
                )
                .unwrap();
                Ok(CachedAttachment {
                    metadata: serde_json::json!({}),
                    local_path,
                })
            })
        };
        let fetcher = Fetcher::new(Cache::new(root.join("cache")), fetch, threads());
        let ask = |request: &str, priority: bool| {
            let (me, request) = (Arc::clone(&fetcher), request.to_owned());
            std::thread::spawn(move || me.fetch("studio", file(&request), priority))
        };
        let mut asked: Vec<_> = (1..=5).map(|n| ask(&format!("req-{n}"), false)).collect();
        wait_for("three reads and two waiting", || {
            started.lock().unwrap().len() == 3 && lock(&fetcher.queue).waiting.len() == 2
        });
        asked.push(ask("req-6", true));
        wait_for("the open to queue", || {
            lock(&fetcher.queue).waiting.len() == 3
        });
        let release = |n: usize| {
            *gate.0.lock().unwrap() += n;
            gate.1.notify_all();
        };
        release(1);
        wait_for("a fourth read", || started.lock().unwrap().len() == 4);
        assert_eq!(
            started.lock().unwrap()[3],
            "req-6",
            "the open goes ahead of the waiting reads"
        );
        release(10);
        for answer in asked {
            assert!(answer.join().unwrap().is_ok());
        }
        assert_eq!(
            most.load(Ordering::SeqCst),
            3,
            "never more than three reads"
        );
        assert_eq!(started.lock().unwrap().len(), 6);
        let queue = lock(&fetcher.queue);
        assert!(queue.jobs.is_empty() && queue.waiting.is_empty() && queue.running == 0);
    }

    #[test]
    fn a_file_read_before_answers_from_its_copy_while_it_lasts() {
        let root = dir("fetch-known");
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Fetcher::new(
            Cache::new(root.join("cache")),
            storing("notes.txt", b"plain words", &calls),
            threads(),
        );
        let first = fetcher.fetch("studio", file("req-1"), false).unwrap();
        assert_eq!(
            first.mime, "text/plain",
            "sniffed when the reply names no type"
        );
        assert_eq!(
            fetcher.fetch("studio", file("req-1"), false).unwrap(),
            first
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "answered from the copy");
        // Another computer's file of the same name is another file.
        fetcher.fetch("mini", file("req-1"), false).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        std::fs::remove_file(fetcher.cache().shown(&first.path).unwrap()).unwrap();
        assert_eq!(
            fetcher.fetch("studio", file("req-1"), false).unwrap(),
            first
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "a vanished copy is read again"
        );
    }

    #[test]
    fn the_files_remembered_stay_bounded() {
        let root = dir("fetch-bound");
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Fetcher::new(
            Cache::new(root.join("cache")),
            storing("a.txt", b"a", &calls),
            Arc::new(|job| job()),
        );
        for n in 0..REMEMBERED + 4 {
            fetcher
                .fetch("studio", file(&format!("req-{n}")), false)
                .unwrap();
        }
        let queue = lock(&fetcher.queue);
        assert_eq!(
            (queue.read.len(), queue.order.len()),
            (REMEMBERED, REMEMBERED)
        );
        assert!(
            !queue.read.contains_key(&key_of("studio", &file("req-0"))),
            "the oldest went first"
        );
    }

    #[test]
    fn a_file_without_a_reference_is_never_read() {
        let root = dir("fetch-unsaved");
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Fetcher::new(
            Cache::new(root.join("cache")),
            storing("a.txt", b"a", &calls),
            threads(),
        );
        assert_eq!(
            fetcher.fetch("studio", file(""), false),
            Err(NO_REFERENCE.to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_failed_read_tells_every_view_and_is_read_again_next_time() {
        let root = dir("fetch-failed");
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let fetch: FetchFn = Arc::new(move |_, _, _| {
            counted.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            Err("Attachment response is too large.".to_owned())
        });
        let fetcher = Fetcher::new(Cache::new(root.join("cache")), fetch, threads());
        let other = Arc::clone(&fetcher);
        let second = std::thread::spawn(move || other.fetch("studio", file("req-1"), false));
        let first = fetcher.fetch("studio", file("req-1"), false);
        assert_eq!(first, Err("Attachment response is too large.".to_owned()));
        assert_eq!(second.join().unwrap(), first);
        let _ = fetcher.fetch("studio", file("req-1"), false);
        assert!(
            calls.load(Ordering::SeqCst) >= 2,
            "nothing failed is remembered"
        );
    }

    /// What an opener was asked: "open" or "reveal", and the file.
    type Asked = Arc<Mutex<Vec<(String, PathBuf)>>>;

    /// An opener that records what it was asked and answers `works`.
    fn opener(works: bool) -> (Opener, Asked) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let (opened, revealed) = (Arc::clone(&asked), Arc::clone(&asked));
        (
            Opener {
                open: Arc::new(move |path| {
                    opened
                        .lock()
                        .unwrap()
                        .push(("open".into(), path.to_path_buf()));
                    works
                }),
                reveal: Arc::new(move |path| {
                    revealed
                        .lock()
                        .unwrap()
                        .push(("reveal".into(), path.to_path_buf()));
                    works
                }),
            },
            asked,
        )
    }

    #[test]
    fn an_activity_attachment_opens_as_another_machines_file() {
        let root = dir("open-activity");
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Fetcher::new(
            Cache::new(root.join("cache")),
            storing("shot.png", PNG, &calls),
            threads(),
        );
        let (works, asked) = opener(true);
        assert_eq!(fetcher.open("studio", file("req-1"), &works), Ok(None));
        let (what, path) = asked.lock().unwrap()[0].clone();
        assert_eq!(what, "open");
        assert!(path.starts_with(fetcher.cache().root()));
        assert_eq!(std::fs::read(&path).unwrap(), PNG);
        let (fails, _) = opener(false);
        assert_eq!(
            fetcher.open("studio", file("req-1"), &fails),
            Err(OPEN_FAILED.to_owned())
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "Open and the thumbnail share the read"
        );
        assert_eq!(
            fetcher.open("studio", file(""), &works),
            Err(NO_REFERENCE.to_owned())
        );
    }

    #[test]
    fn a_program_is_shown_in_the_file_manager_instead() {
        let root = dir("open-program");
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher = Fetcher::new(
            Cache::new(root.join("cache")),
            storing("setup.exe", b"MZ program", &calls),
            threads(),
        );
        let (works, asked) = opener(true);
        let shown = fetcher
            .open("studio", file("req-1"), &works)
            .unwrap()
            .unwrap();
        assert_eq!(shown, "setup.exe can run programs, so it is shown in the file manager instead of being opened.");
        assert_eq!(asked.lock().unwrap()[0].0, "reveal");
        let (fails, _) = opener(false);
        let error = fetcher.open("studio", file("req-1"), &fails).unwrap_err();
        assert!(error.starts_with("setup.exe can run programs, and the file manager could not show it. Its copy is at "), "{error}");
    }

    #[test]
    fn a_composer_attachment_opens_its_copy_while_the_list_is_the_one_shown() {
        let root = dir("open-draft");
        let original = root.join("notes.txt");
        std::fs::write(&original, b"draft notes").unwrap();
        let (c, _) = hub(&root);
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        c.attach_paths(view.handle, vec![original.clone()], Selection::default());
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        let path = view.attachments[0].path.clone();
        let (works, asked) = opener(true);
        assert_eq!(open_draft(&c, &path, &works), Ok(None));
        let (what, copy) = asked.lock().unwrap()[0].clone();
        assert_eq!(what, "open");
        assert!(
            copy.starts_with(c.cache().root()),
            "the cache's copy, never the original"
        );
        assert_eq!(std::fs::read(&copy).unwrap(), b"draft notes");
        let (fails, _) = opener(false);
        assert_eq!(
            open_draft(&c, &path, &fails),
            Err(DRAFT_OPEN_FAILED.to_owned())
        );
        // A list that changed, or no such address, opens nothing.
        c.remove(view.handle, 0);
        until(&c, |v| v.attachments.is_empty());
        let (quiet, untouched) = opener(true);
        for gone in [path.as_str(), "draft/x/1/0", "cache/abc/notes.txt", ""] {
            assert_eq!(open_draft(&c, gone, &quiet), Ok(None), "{gone}");
        }
        assert!(untouched.lock().unwrap().is_empty());
    }
}
