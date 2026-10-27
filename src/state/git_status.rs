//! Read-only checkout/publication evidence, separate from fleet and agent polling.
use super::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::time::{Duration, Instant};
use worktrees::git;

/// The bound of one local git read: its process start (about 0.1 s on Windows)
/// and its work. Each read has its own, so a slow read never leaves the reads
/// after it already expired. 2 s by the owner's decision (#526): while other
/// agents build, one git status takes up to 1.2 s here, and 0.7 s made the
/// desktop's evidence "timeout" on many refreshes.
const LOCAL_READ: Duration = Duration::from_secs(2);
/// One read of the changes: a diff or the untracked files.
const CHANGES_READ: Duration = Duration::from_secs(2);
/// The remote check: its endpoint and one ls-remote.
const REMOTE_READ: Duration = Duration::from_secs(3);
/// The longest one `inspect` can take: five local reads, two changes reads,
/// the remote check and the recheck after it. Its callers wait longer: the
/// desktop 25 s (30 s over SSH) and the phone connector 25 s.
#[cfg(test)]
const INSPECT_LONGEST: Duration = Duration::from_secs(5 * 2 + 2 * 2 + 3 + 2);

/// The bound of a read: the product's own, or the one a test sets for itself.
fn bound(product: Duration) -> Duration {
    #[cfg(test)]
    if let Some(bound) = tests::READ_BOUND.with(std::cell::Cell::get) {
        return bound;
    }
    product
}

fn local() -> Instant {
    Instant::now() + bound(LOCAL_READ)
}

fn text(cwd: &Path, args: &[&str], deadline: Instant) -> Option<String> {
    String::from_utf8(git(cwd, args, deadline).ok()?).ok().map(|s| s.trim().to_owned())
}

fn oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn parse_status(bytes: &[u8]) -> Option<Value> {
    let mut fields = bytes.split(|b| *b == 0);
    let mut data = json!({"changed_files":0,"conflicts":0,"ahead":null,"behind":null});
    let mut changed = 0;
    let mut conflicts = 0;
    while let Some(field) = fields.next() {
        if field.is_empty() { continue; }
        if field.starts_with(b"# ") {
            let line = std::str::from_utf8(field).ok()?;
            if let Some(head) = line.strip_prefix("# branch.oid ") {
                if head != "(initial)" && !oid(head) { return None; }
                data["head"] = json!(head);
                data["unborn"] = json!(head == "(initial)");
            } else if let Some(branch) = line.strip_prefix("# branch.head ") {
                data["detached"] = json!(branch == "(detached)");
                data["branch"] = json!(if branch == "(detached)" { "" } else { branch });
            } else if let Some(upstream) = line.strip_prefix("# branch.upstream ") {
                data["upstream"] = json!(upstream);
            } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
                let (ahead, behind) = ab.split_once(' ')?;
                data["ahead"] = json!(ahead.strip_prefix('+')?.parse::<u64>().ok()?);
                data["behind"] = json!(behind.strip_prefix('-')?.parse::<u64>().ok()?);
            }
        } else {
            match field.first()? {
                b'1' | b'?' => changed += 1,
                b'2' => { changed += 1; fields.next()?; }, // Rename: one path, with a second NUL field.
                b'u' => { changed += 1; conflicts += 1; },
                _ => return None,
            }
        }
    }
    if !data["head"].is_string() || !data["branch"].is_string() { return None; }
    data["changed_files"] = json!(changed);
    data["conflicts"] = json!(conflicts);
    Some(data)
}

/// U25 part 6: at most this many changed files are listed with their line
/// counts; the totals cover every tracked change.
const CHANGED_FILES: usize = 50;
/// A listed path is clipped so the status stays small (the desktop reads 32 KiB).
const CHANGED_PATH: usize = 300;
/// `hgs git-diff` returns at most this much of the diff.
const DIFF_BYTES: usize = 256 * 1024;

fn clipped_path(path: &str) -> String {
    if path.chars().count() <= CHANGED_PATH { return path.to_owned(); }
    let tail: String = path.chars().rev().take(CHANGED_PATH - 1).collect::<Vec<_>>().into_iter().rev().collect();
    format!("…{tail}")
}

/// `git diff --numstat -z HEAD`: each tracked change against HEAD with its
/// added and deleted lines (none for a binary file); a rename names its new
/// path. Totals count every file, the list keeps the first CHANGED_FILES.
fn parse_numstat(bytes: &[u8]) -> Option<Value> {
    let mut fields = bytes.split(|b| *b == 0);
    let (mut added, mut deleted, mut count) = (0u64, 0u64, 0u64);
    let mut files = Vec::new();
    while let Some(field) = fields.next() {
        if field.is_empty() { continue; }
        let line = std::str::from_utf8(field).ok()?;
        let mut parts = line.splitn(3, '\t');
        let (a, d, path) = (parts.next()?, parts.next()?, parts.next()?);
        // A rename's paths follow as two NUL fields: old, then new.
        let path = if path.is_empty() {
            fields.next()?;
            String::from_utf8_lossy(fields.next()?).into_owned()
        } else {
            path.to_owned()
        };
        let a = if a == "-" { None } else { Some(a.parse::<u64>().ok()?) };
        let d = if d == "-" { None } else { Some(d.parse::<u64>().ok()?) };
        added += a.unwrap_or(0);
        deleted += d.unwrap_or(0);
        count += 1;
        if files.len() < CHANGED_FILES {
            files.push(json!({"path":clipped_path(&path),"added":a,"deleted":d}));
        }
    }
    Some(json!({"added":added,"deleted":deleted,"files":files,"tracked_files":count}))
}

/// U25 part 6 (spec 6, Changes): the checkout's changes against HEAD with line
/// counts, then the untracked files (no counts), at most CHANGED_FILES in all.
/// Read-only: git runs directly, without a shell, and never writes.
fn changes(path: &Path, unborn: bool) -> Option<Value> {
    #[cfg(test)]
    std::thread::sleep(tests::SLOW_CHANGES.with(std::cell::Cell::get));
    let mut changes = if unborn {
        json!({"added":0,"deleted":0,"files":[],"tracked_files":0})
    } else {
        parse_numstat(&git(path, &["diff", "--numstat", "-z", "--no-renames", "--no-ext-diff", "HEAD", "--"], Instant::now() + bound(CHANGES_READ)).ok()?)?
    };
    let untracked = git(path, &["ls-files", "--others", "--exclude-standard", "-z"], Instant::now() + bound(CHANGES_READ)).ok()?;
    let mut count = 0u64;
    let files = changes["files"].as_array_mut()?;
    for name in untracked.split(|b| *b == 0).filter(|name| !name.is_empty()) {
        count += 1;
        if files.len() < CHANGED_FILES {
            files.push(json!({"path":clipped_path(&String::from_utf8_lossy(name)),"added":null,"deleted":null,"untracked":true}));
        }
    }
    changes["untracked_files"] = json!(count);
    let total = changes["tracked_files"].as_u64().unwrap_or(0) + count;
    changes["truncated"] = json!(total > CHANGED_FILES as u64);
    Some(changes)
}

/// `hgs git-diff --path P --json`: the checkout's diff against HEAD as text,
/// at most DIFF_BYTES. Read-only, run with git directly.
pub(super) fn diff(path: &Path) -> Value {
    let deadline = Instant::now() + bound(Duration::from_secs(5));
    let mut data = json!({"state":"unavailable","path":path});
    let Some(path) = crate::platform::canonical(path).ok().filter(|p| p.is_dir()) else {
        data["state"] = json!("folder_unavailable"); return data;
    };
    data["path"] = json!(path);
    match git(&path, &["diff", "--no-color", "--no-ext-diff", "--no-textconv", "HEAD", "--"], deadline) {
        Ok(bytes) => {
            let truncated = bytes.len() > DIFF_BYTES;
            let mut end = bytes.len().min(DIFF_BYTES);
            // Never cut inside a UTF-8 sequence.
            while end > 0 && end < bytes.len() && (bytes[end] & 0xC0) == 0x80 { end -= 1; }
            data["diff"] = json!(String::from_utf8_lossy(&bytes[..end]));
            data["truncated"] = json!(truncated);
            data["bytes"] = json!(bytes.len());
            data["state"] = json!("ok");
        }
        Err(error) => { data["state"] = json!(error.state); }
    }
    data
}

fn remote_check(cwd: &Path, common: &Path, remote: &str, reference: &str, tracking: &str, deadline: Instant) -> Value {
    // Include the configured endpoint and observed tracking OID in the identity.
    // A push/fetch or remote reconfiguration invalidates old verification.
    let Some(endpoint) = text(cwd, &["remote", "get-url", "--", remote], deadline) else {
        return json!({"state":"unavailable","at":now()});
    };
    let identity = format!("{}\0{remote}\0{reference}\0{tracking}\0{endpoint}", common.display());
    let cache = root().join("git-status-cache").join(format!("{:x}.json", Sha256::digest(identity.as_bytes())));
    if let Some(cached) = fs::metadata(&cache).ok().filter(|m| m.len() < 4096)
        .and_then(|_| fs::read(&cache).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        let age = now() - cached["at"].as_f64().unwrap_or(0.0);
        if (0.0..60.0).contains(&age) { return cached; }
    }
    let reported = text(cwd, &["-c", "credential.interactive=false", "ls-remote", "--refs", "--", remote, reference], deadline);
    let mut result = json!({"state":"unavailable","at":now()});
    if let Some(reported) = reported {
        // ls-remote patterns can match more than one ref. Accept only the exact one.
        let matches: Vec<_> = reported.lines().filter_map(|line| line.split_once('\t'))
            .filter(|(hash, name)| *name == reference && oid(hash)).collect();
        result["state"] = json!(match matches.as_slice() {
            [(hash, _)] if *hash == tracking => "verified",
            [(_, _)] => "changed",
            [] if reported.is_empty() => "missing",
            _ => "unavailable",
        });
    }
    let _ = atomic(&cache, &result.to_string());
    result
}

pub(super) fn inspect(path: &Path) -> Value {
    let mut data = json!({"state":"unavailable","path":path,"sampled_at":now(),"remote_state":"unknown"});
    let Some(path) = crate::platform::canonical(path).ok().filter(|p| p.is_dir()) else {
        data["state"] = json!("folder_unavailable"); return data;
    };
    data["path"] = json!(path);
    let bytes = match git(&path, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"], local()) {
        Ok(bytes) => bytes,
        Err(error) => { data["state"] = json!(error.state); return data; }
    };
    let Some(status) = parse_status(&bytes) else { return data; };
    data.as_object_mut().unwrap().extend(status.as_object().unwrap().clone());
    if status["changed_files"].as_u64().is_some_and(|changed| changed > 0) {
        if let Some(changes) = changes(&path, status["unborn"] == true) {
            data["changes"] = changes;
        }
    }
    let Some(root_path) = text(&path, &["rev-parse", "--show-toplevel"], local()) else { return data; };
    let Some(common) = text(&path, &["rev-parse", "--path-format=absolute", "--git-common-dir"], local()) else { return data; };
    data["root"] = json!(root_path);
    data["common_dir"] = json!(common);
    data["state"] = json!("ok");
    if data["detached"] == true {
        data["remote_state"] = json!("detached"); return data;
    }
    let branch = format!("refs/heads/{}", string(&data, "branch"));
    // A failed or timed-out lookup is no evidence that no upstream is
    // configured: keep remote_state unknown.
    let Some(upstream) = text(&path, &["for-each-ref", "--format=%(upstream:remotename)%00%(upstream:remoteref)", &branch], local()) else {
        return data;
    };
    let Some((remote, reference)) = upstream.split_once('\0') else {
        data["remote_state"] = json!("no_upstream"); return data;
    };
    if remote.is_empty() || reference.is_empty() || remote == "." {
        data["remote_state"] = json!(if remote == "." { "local_upstream" } else { "no_upstream" }); return data;
    }
    data["remote"] = json!(remote);
    data["remote_ref"] = json!(reference);
    // Only a completed lookup that finds no tracking ref compares the remote with
    // an empty OID; a failed or timed-out lookup keeps remote_state unknown.
    let tracking = match git(&path, &["rev-parse", "--verify", "--quiet", "@{upstream}"], local()) {
        Ok(bytes) => String::from_utf8(bytes).ok().map(|s| s.trim().to_owned()).filter(|s| oid(s)),
        // With --quiet, a name that does not resolve exits 1 without a message.
        Err(e) if e.code == Some(1) && e.detail.is_empty() => Some(String::new()),
        Err(_) => None,
    };
    let Some(tracking) = tracking else { return data; };
    let checked = remote_check(&path, Path::new(&common), remote, reference, &tracking, Instant::now() + REMOTE_READ);
    data["remote_state"] = checked["state"].clone();
    data["remote_checked_at"] = checked["at"].clone();
    data["remote_age_seconds"] = json!((now() - checked["at"].as_f64().unwrap_or(0.0)).max(0.0));
    // Recheck after network IO: do not certify a checkout that switched branches
    // or changed while verification was in flight.
    match git(&path, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"], local())
        .ok().and_then(|b| parse_status(&b)) {
        Some(current) if current == status => {},
        Some(_) => { data["state"] = json!("changed_during_check"); },
        None => { data["state"] = json!("unavailable"); },
    }
    data
}

pub(super) fn dispatch(args: &[String]) -> Result<i32> {
    println!("{}", inspect(Path::new(&folder(args, "git-status")?)));
    Ok(0)
}

/// `hgs git-diff --path PATH --json` (U25 part 6).
pub(super) fn dispatch_diff(args: &[String]) -> Result<i32> {
    println!("{}", diff(Path::new(&folder(args, "git-diff")?)));
    Ok(0)
}

fn folder(args: &[String], command: &str) -> Result<String> {
    let usage = format!("usage: hgs {command} --path PATH --json");
    let mut path = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => {},
            "--path" if path.is_none() => { i += 1; path = args.get(i); },
            _ => return Err(usage),
        }
        i += 1;
    }
    let path = path.ok_or(usage)?;
    if !Path::new(path).is_absolute() || path.chars().any(char::is_control) {
        return Err("Git status requires an absolute folder path".into());
    }
    Ok(path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// How long reading the changes takes on this thread: a slow diff.
        pub(super) static SLOW_CHANGES: std::cell::Cell<Duration> = const { std::cell::Cell::new(Duration::ZERO) };
        /// The bound of each read on this thread, in place of the product's.
        pub(super) static READ_BOUND: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
    }

    /// The product bounds each read for a responsive desktop and says
    /// "timeout" when a busy computer is slower (here a plain git status takes
    /// 0.1 to 1.2 s while other agents build); the desktop reads again on its
    /// next refresh. A test of what the reads return owns its bound instead,
    /// as it owns its clock: `bound` for each read on this thread.
    fn with_bound<T>(bound: Duration, run: impl FnOnce() -> T) -> T {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                READ_BOUND.with(|value| value.set(None));
                SLOW_CHANGES.with(|value| value.set(Duration::ZERO));
            }
        }
        let _reset = Reset;
        READ_BOUND.with(|value| value.set(Some(bound)));
        run()
    }
    /// A read that has not finished in this long is a broken git, not a busy
    /// computer.
    const PATIENT: Duration = Duration::from_secs(20);

    /// A checkout with changes in a fresh repository of its own (git never
    /// looks above the temp folder).
    fn changed_checkout() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git").arg("-C").arg(&repo).args(args)
                .env("GIT_CEILING_DIRECTORIES", dir.path()).status().unwrap();
            assert!(status.success(), "{args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["-c", "user.name=T", "-c", "user.email=t@example.com", "commit", "-q", "--allow-empty", "-m", "start"]);
        fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["-c", "user.name=T", "-c", "user.email=t@example.com", "commit", "-q", "-m", "a"]);
        fs::write(repo.join("a.txt"), "one\nthree\nfour\n").unwrap();
        fs::write(repo.join("new.txt"), "fresh\n").unwrap();
        (dir, repo)
    }

    #[test]
    fn a_slow_changes_read_leaves_the_later_reads_their_own_bound() {
        let (_dir, repo) = changed_checkout();
        // Reading the changes takes longer than a read's bound: when the reads
        // shared one deadline, every read after the changes started expired
        // and the checkout read as unavailable. Each read keeps half of the
        // patient bound, which a busy computer's git stays well within.
        let bound = PATIENT / 2;
        let status = with_bound(bound, || {
            SLOW_CHANGES.with(|slow| slow.set(bound + Duration::from_millis(500)));
            inspect(&repo)
        });
        assert_eq!(status["state"], "ok", "{status}");
        assert!(status["root"].is_string() && status["common_dir"].is_string(), "{status}");
        assert_eq!(status["remote_state"], "no_upstream", "{status}");
    }

    #[test]
    fn every_read_has_its_own_bound_and_the_callers_wait_for_all_of_them() {
        assert_eq!((LOCAL_READ, CHANGES_READ, REMOTE_READ), (Duration::from_secs(2), Duration::from_secs(2), Duration::from_secs(3)));
        assert_eq!(INSPECT_LONGEST, LOCAL_READ * 5 + CHANGES_READ * 2 + REMOTE_READ + LOCAL_READ);
        // The desktop's and the phone connector's timeouts for git-status.
        assert!(INSPECT_LONGEST < Duration::from_secs(25));
    }

    #[test]
    fn nul_paths_and_renames_are_counted_once() {
        let bytes = b"# branch.oid aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0# branch.head feature/status\0# branch.upstream origin/main\0# branch.ab +2 -3\x001 MM ignored metadata file\x002 R. ignored metadata new\nname\0old\nname\0? non-utf8-\xff\0u UU conflicted\0";
        let status = parse_status(bytes).unwrap();
        assert_eq!(status["changed_files"], 4);
        assert_eq!(status["conflicts"], 1);
        assert_eq!((status["ahead"].as_u64(), status["behind"].as_u64()), (Some(2), Some(3)));
        assert!(parse_status(b"# branch.head main\0").is_none());
        assert!(parse_status(b"# branch.oid bad\0# branch.head main\0").is_none());
    }

    #[test]
    fn numstat_counts_lines_binaries_and_renames_and_keeps_fifty() {
        let bytes = b"12\t3\tsrc/a.rs\0-\t-\tlogo.png\x000\t4\t\0old/name.rs\0new/name.rs\0";
        let changes = parse_numstat(bytes).unwrap();
        assert_eq!((changes["added"].as_u64(), changes["deleted"].as_u64(), changes["tracked_files"].as_u64()), (Some(12), Some(7), Some(3)));
        assert_eq!(changes["files"][0], json!({"path":"src/a.rs","added":12,"deleted":3}));
        assert_eq!(changes["files"][1], json!({"path":"logo.png","added":null,"deleted":null}));
        assert_eq!(changes["files"][2]["path"], "new/name.rs");
        let many: Vec<u8> = (0..80).flat_map(|n| format!("1\t1\tf{n}.txt\0").into_bytes()).collect();
        let changes = parse_numstat(&many).unwrap();
        assert_eq!(changes["files"].as_array().unwrap().len(), CHANGED_FILES);
        assert_eq!(changes["tracked_files"], 80);
        assert_eq!(changes["added"], 80);
        assert!(parse_numstat(b"x\t1\tbad\0").is_none());
        let long = "d/".repeat(400);
        assert!(clipped_path(&long).chars().count() <= CHANGED_PATH);
    }

    /// U25 part 6: the Changes card's data from a real checkout - git-status
    /// lists each change with its lines, git-diff returns the text - and
    /// neither writes (the index and the files are unchanged).
    #[test]
    fn a_checkouts_changes_and_diff_are_read_without_writing() {
        let (dir, repo) = changed_checkout();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git").arg("-C").arg(&repo).args(args)
                .env("GIT_CEILING_DIRECTORIES", dir.path()).status().unwrap();
            assert!(status.success(), "{args:?}");
        };
        let index = fs::read(repo.join(".git").join("index")).unwrap();
        let status = with_bound(PATIENT, || inspect(&repo));
        assert_eq!(status["state"], "ok", "{status}");
        let changes = &status["changes"];
        assert_eq!((changes["added"].as_u64(), changes["deleted"].as_u64()), (Some(2), Some(1)), "{status}");
        assert_eq!(changes["files"][0], json!({"path":"a.txt","added":2,"deleted":1}));
        assert_eq!(changes["files"][1], json!({"path":"new.txt","added":null,"deleted":null,"untracked":true}));
        assert_eq!(changes["untracked_files"], 1);
        assert_eq!(changes["truncated"], false);
        let diff = with_bound(PATIENT, || diff(&repo));
        assert_eq!(diff["state"], "ok");
        assert!(diff["diff"].as_str().unwrap().contains("+three"), "{diff}");
        assert_eq!(diff["truncated"], false);
        assert_eq!(fs::read(repo.join(".git").join("index")).unwrap(), index, "the index is not rewritten");
        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "one\nthree\nfour\n");
        // A clean checkout has no changes entry; a missing folder says so.
        fs::remove_file(repo.join("new.txt")).unwrap();
        run(&["checkout", "-q", "--", "a.txt"]);
        let clean = with_bound(PATIENT, || inspect(&repo));
        assert!(clean["state"] == "ok" && clean.get("changes").is_none(), "{clean}");
        assert_eq!(super::diff(&dir.path().join("missing"))["state"], "folder_unavailable");
    }
}
