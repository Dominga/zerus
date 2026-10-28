//! Git worktrees, ported from the Qt client: the folder's worktree catalog
//! and its saved snapshots, the verified catalog New session creates from,
//! and creating a worktree with its receipt.
//!
//! Every call blocks; callers run it off the UI thread. Two reads of the same
//! folder share one call in the caller.
//!
//! Reference: `tray/src/HgsClient.cpp` (`requestWorktrees`, `worktreeSnapshot`,
//! `saveWorktreeSnapshots`, `worktreeCacheKey`, `invalidateWorktreeCache`,
//! `createWorktree`, `worktreeReceipt`, `cleanupError`, `guardMachine`).
use crate::actions::{Exit, Invocation};
use crate::ComputerId;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Snapshots kept, the oldest `sampled_at` going first.
pub const SNAPSHOTS: usize = 64;
/// The saved snapshots' size.
pub const SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;
/// A snapshot younger than this is used without reading again.
pub const FRESH_SECONDS: f64 = 30.0;
/// The largest catalog reply.
pub const CATALOG_LIMIT: usize = 2 * 1024 * 1024;

fn hgs_args(computer: &ComputerId, words: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(words.iter().map(|word| (*word).to_owned()));
    args
}

/// Names the computer a call must reach (`guardMachine`): `--expect-machine
/// ID` after the `@alias`, so a reused alias never acts on another computer.
pub fn guard(args: &mut Vec<String>, expected_machine: &str) {
    if expected_machine.is_empty() {
        return;
    }
    let at = usize::from(args.first().is_some_and(|first| first.starts_with('@')));
    args.insert(at, "--expect-machine".into());
    args.insert(at + 1, expected_machine.into());
}

fn invocation(args: Vec<String>, seconds: u64) -> Invocation {
    Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(seconds),
        request_id: None,
    }
}

/// `hgs [@host] worktrees --path P --json [--refresh]`, 6 s here and 15 s on
/// a peer.
pub fn catalog_invocation(computer: &ComputerId, path: &str, refresh: bool) -> Invocation {
    let mut args = hgs_args(computer, &["worktrees", "--path", path, "--json"]);
    if refresh {
        args.push("--refresh".into());
    }
    invocation(args, if computer.is_local() { 6 } else { 15 })
}

/// The verified catalog New session creates from: `hgs [@host]
/// --expect-machine M worktrees --creation --path P --refresh --json`, 15 s.
pub fn verified_invocation(
    computer: &ComputerId,
    path: &str,
    expected_machine: &str,
) -> Invocation {
    let mut args = hgs_args(
        computer,
        &[
            "worktrees",
            "--creation",
            "--path",
            path,
            "--refresh",
            "--json",
        ],
    );
    guard(&mut args, expected_machine);
    invocation(args, 15)
}

fn object(exit: Exit) -> Result<Map<String, Value>, String> {
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => match serde_json::from_slice::<Value>(&stdout) {
            Ok(Value::Object(object)) => Ok(object),
            _ => Ok(Map::new()),
        },
        Exit::Code { code, stderr, .. } => Err(failure(&stderr, Some(code))),
        Exit::Abnormal { stderr } => Err(failure(&stderr, None)),
        Exit::TimedOut => Err("timed out".into()),
        Exit::StartFailed(error) => Err(error),
        Exit::WriteFailed => Err("hgs did not take the request".into()),
    }
}

fn failure(stderr: &str, code: Option<i32>) -> String {
    let text = stderr.trim();
    if !text.is_empty() {
        text.chars().take(4000).collect()
    } else {
        match code {
            Some(code) => format!("exit code {code}"),
            None => "hgs ended abnormally".into(),
        }
    }
}

/// Runs a catalog read bounded at 2 MiB. As in Qt, a larger reply reads as
/// an empty object, which [`Catalogs::accept`] and [`verified`] refuse.
pub fn run_catalog(hgs: &Path, invocation: &Invocation) -> Exit {
    match crate::run::run_limited(hgs, invocation, &[], CATALOG_LIMIT) {
        Ok(exit) => exit,
        Err(_) => Exit::Code {
            code: 0,
            stdout: b"{}".to_vec(),
            stderr: String::new(),
        },
    }
}

/// The verified catalog's reply: an object with a `worktrees` array and a
/// string `state`.
pub fn verified(exit: Exit) -> Result<Value, String> {
    let object = object(exit)?;
    if object.get("worktrees").is_some_and(Value::is_array)
        && object.get("state").is_some_and(Value::is_string)
    {
        Ok(Value::Object(object))
    } else {
        Err("Invalid verified worktree catalog.".into())
    }
}

/// A read's view of the cache when it started: a reply is stored only while
/// neither its repository nor its computer changed meanwhile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    repository: String,
    repository_generation: u64,
    host: String,
    host_generation: u64,
}

/// The folder catalogs this client saw, persisted across restarts. Keys hold
/// the hgs executable, the computer and the path, so another computer's
/// identical path is unrelated.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalogs {
    executable: String,
    snapshots: Map<String, Value>,
    generations: HashMap<String, u64>,
}

fn host_key(host: &str) -> String {
    serde_json::to_string(&[host]).unwrap_or_default()
}

impl Catalogs {
    /// The saved snapshots of `executable`'s catalogs (the JSON object a
    /// previous [`Catalogs::saved`] wrote, if any).
    pub fn new(executable: &str, saved: Option<Value>) -> Catalogs {
        let snapshots = match saved {
            Some(Value::Object(snapshots)) => snapshots,
            _ => Map::new(),
        };
        let mut catalogs = Catalogs {
            executable: executable.into(),
            snapshots,
            generations: HashMap::new(),
        };
        catalogs.bound();
        catalogs
    }

    /// The snapshots to persist (at most 64 entries and 8 MiB).
    pub fn saved(&self) -> Value {
        Value::Object(self.snapshots.clone())
    }

    fn key(&self, host: &str, path: &str) -> String {
        Sha256::digest(format!("{}\n{host}\n{path}", self.executable).as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn bound(&mut self) {
        while self.snapshots.len() > SNAPSHOTS
            || Value::Object(self.snapshots.clone()).to_string().len() > SNAPSHOT_BYTES
        {
            let oldest = self
                .snapshots
                .iter()
                .min_by(|a, b| {
                    let at = |value: &Value| value["sampled_at"].as_f64().unwrap_or(0.0);
                    at(a.1).total_cmp(&at(b.1))
                })
                .map(|(key, _)| key.clone());
            match oldest {
                Some(oldest) => {
                    self.snapshots.remove(&oldest);
                }
                None => break,
            }
        }
    }

    /// The saved snapshot of a folder, fresh or not.
    pub fn snapshot(&self, host: &str, path: &str) -> Option<&Value> {
        self.snapshots.get(&self.key(host, path))
    }

    /// The snapshot to use without reading: sampled under 30 s ago and not
    /// marked stale.
    pub fn fresh(&self, host: &str, path: &str, now: f64) -> Option<&Value> {
        self.snapshot(host, path).filter(|snapshot| {
            let age = now - snapshot["sampled_at"].as_f64().unwrap_or(0.0);
            snapshot["stale"].as_bool() != Some(true) && (0.0..FRESH_SECONDS).contains(&age)
        })
    }

    /// The generations a read of this folder starts with.
    pub fn stamp(&self, host: &str, path: &str) -> Stamp {
        let common_dir = self
            .snapshot(host, path)
            .and_then(|snapshot| snapshot["common_dir"].as_str())
            .unwrap_or("")
            .to_owned();
        let repository = format!("{host}\n{common_dir}");
        let host = host_key(host);
        Stamp {
            repository_generation: self.generations.get(&repository).copied().unwrap_or(0),
            host_generation: self.generations.get(&host).copied().unwrap_or(0),
            repository,
            host,
        }
    }

    /// A catalog read's reply, kept for its folder and, when the catalog is
    /// `ok`, for each of its worktrees' roots too.
    pub fn accept(
        &mut self,
        host: &str,
        path: &str,
        stamp: &Stamp,
        exit: Exit,
        now: f64,
    ) -> Result<Value, String> {
        let mut object = object(exit)?;
        if self
            .generations
            .get(&stamp.repository)
            .copied()
            .unwrap_or(0)
            != stamp.repository_generation
            || self.generations.get(&stamp.host).copied().unwrap_or(0) != stamp.host_generation
        {
            return Err("Folder changed. Refresh its catalog.".into());
        }
        object.insert("received_at".into(), Value::from(now.floor()));
        if !object.get("worktrees").is_some_and(Value::is_array)
            || !object.get("state").is_some_and(Value::is_string)
        {
            return Err("Invalid worktree catalog response".into());
        }
        let data = Value::Object(object);
        let key = self.key(host, path);
        self.snapshots.insert(key, data.clone());
        if data["state"] == "ok" {
            for worktree in data["worktrees"].as_array().into_iter().flatten() {
                let Some(root) = worktree["path"].as_str() else {
                    continue;
                };
                let mut sibling = data.clone();
                sibling["path"] = Value::from(root);
                sibling["selected_root"] = Value::from(root);
                let key = self.key(host, root);
                self.snapshots.insert(key, sibling);
            }
        }
        self.bound();
        Ok(data)
    }

    /// A worktree of the repository was created or removed: reads under way
    /// fail, and its saved snapshots are stale.
    pub fn invalidate(&mut self, host: &str, common_dir: &str) {
        *self.generations.entry(host_key(host)).or_default() += 1;
        *self
            .generations
            .entry(format!("{host}\n{common_dir}"))
            .or_default() += 1;
        let keys: Vec<String> = self
            .snapshots
            .iter()
            .filter(|(key, value)| {
                value["common_dir"].as_str() == Some(common_dir)
                    && **key == self.key(host, value["path"].as_str().unwrap_or(""))
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            if let Some(value) = self.snapshots.get_mut(&key) {
                value["stale"] = Value::Bool(true);
            }
        }
    }
}

/// `hgs [@host] --expect-machine M worktrees create --path P --common-dir C
/// --branch B --base BASE --destination D --request-id ID --json
/// [--receipt]`, 330 s.
#[allow(clippy::too_many_arguments)]
pub fn create_invocation(
    computer: &ComputerId,
    path: &str,
    common_dir: &str,
    branch: &str,
    base: &str,
    destination: &str,
    request_id: &str,
    receipt: bool,
    expected_machine: &str,
) -> Invocation {
    let mut args = hgs_args(
        computer,
        &[
            "worktrees",
            "create",
            "--path",
            path,
            "--common-dir",
            common_dir,
            "--branch",
            branch,
            "--base",
            base,
            "--destination",
            destination,
            "--request-id",
            request_id,
            "--json",
        ],
    );
    if receipt {
        args.push("--receipt".into());
    }
    guard(&mut args, expected_machine);
    invocation(args, 330)
}

/// Whether the worktree was created as asked. The caller invalidates the
/// repository's catalogs on every outcome.
pub fn created(
    exit: Exit,
    request_id: &str,
    branch: &str,
    common_dir: &str,
    receipt: bool,
    destination: &str,
) -> Result<Value, String> {
    let data = match object(exit) {
        Ok(data) => data,
        Err(error) => {
            return Err(format!(
                "{error}\nCheck {destination} before retrying. Any created worktree is kept."
            ))
        }
    };
    let field = |key: &str| data.get(key).and_then(Value::as_str);
    if receipt && field("request_id") == Some(request_id) && field("status") != Some("created") {
        return Err(field("detail")
            .unwrap_or("Worktree creation is not confirmed. Check its original result.")
            .to_owned());
    }
    let absolute =
        field("path").is_some_and(|path| Path::new(path).is_absolute() || path.starts_with('/'));
    if field("request_id") != Some(request_id)
        || field("status") != Some("created")
        || field("branch") != Some(branch)
        || field("common_dir") != Some(common_dir)
        || !absolute
    {
        return Err("Creation could not be confirmed. Refresh the catalog and check the destination before retrying.".into());
    }
    Ok(Value::Object(data))
}

/// `hgs [@host] --expect-machine M worktrees ACTION --request-id ID --json`,
/// 15 s: the receipt of an earlier create or remove.
pub fn receipt_invocation(
    computer: &ComputerId,
    action: &str,
    request_id: &str,
    expected_machine: &str,
) -> Invocation {
    let mut args = hgs_args(
        computer,
        &["worktrees", action, "--request-id", request_id, "--json"],
    );
    guard(&mut args, expected_machine);
    invocation(args, 15)
}

/// A receipt names the same request with `schema` 1 and a string `status`.
pub fn receipt(exit: Exit, request_id: &str) -> Result<Value, String> {
    let data = object(exit)?;
    if data.get("schema") == Some(&Value::from(1))
        && data.get("request_id").and_then(Value::as_str) == Some(request_id)
        && data.get("status").is_some_and(Value::is_string)
    {
        Ok(Value::Object(data))
    } else {
        Err("Invalid worktree receipt.".into())
    }
}

/// An older hgs prints the catalog usage for the review and remove actions.
pub fn cleanup_error(host: &str, detail: &str) -> String {
    if detail.contains("usage: hgs worktrees --path") {
        if host.is_empty() {
            "Update Zerus on this computer to clean up worktrees.".into()
        } else {
            format!("Update Zerus on {host} to clean up worktrees.")
        }
    } else {
        detail.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok(value: Value) -> Exit {
        Exit::Code {
            code: 0,
            stdout: value.to_string().into_bytes(),
            stderr: String::new(),
        }
    }

    fn catalog(sampled_at: f64) -> Value {
        json!({"state": "ok", "common_dir": "/home/example/repo/.git", "path": "/home/example/repo", "sampled_at": sampled_at,
               "worktrees": [{"path": "/home/example/repo"}, {"path": "/home/example/repo-work-1"}]})
    }

    #[test]
    fn calls_carry_qt_arguments_and_the_machine_guard() {
        let studio = ComputerId::from("studio");
        let read = catalog_invocation(&studio, "/home/example/repo", true);
        assert_eq!(
            read.args,
            [
                "@studio",
                "worktrees",
                "--path",
                "/home/example/repo",
                "--json",
                "--refresh"
            ]
        );
        assert_eq!(read.timeout, Duration::from_secs(15));
        assert_eq!(
            catalog_invocation(&ComputerId::local(), "/r", false).timeout,
            Duration::from_secs(6)
        );
        let verified = verified_invocation(&studio, "/r", "machine-1");
        assert_eq!(
            verified.args,
            [
                "@studio",
                "--expect-machine",
                "machine-1",
                "worktrees",
                "--creation",
                "--path",
                "/r",
                "--refresh",
                "--json"
            ]
        );
        let create = create_invocation(
            &ComputerId::local(),
            "/r",
            "/r/.git",
            "work-1a2b3c4d",
            "HEAD",
            "/r-work-1a2b3c4d",
            "req-1",
            true,
            "m",
        );
        assert_eq!(
            create.args,
            [
                "--expect-machine",
                "m",
                "worktrees",
                "create",
                "--path",
                "/r",
                "--common-dir",
                "/r/.git",
                "--branch",
                "work-1a2b3c4d",
                "--base",
                "HEAD",
                "--destination",
                "/r-work-1a2b3c4d",
                "--request-id",
                "req-1",
                "--json",
                "--receipt"
            ]
        );
        assert_eq!(create.timeout, Duration::from_secs(330));
        assert_eq!(
            receipt_invocation(&studio, "create", "req-1", "").args,
            [
                "@studio",
                "worktrees",
                "create",
                "--request-id",
                "req-1",
                "--json"
            ]
        );
    }

    #[test]
    fn snapshots_are_fresh_for_30_s_shared_with_siblings_and_invalidated() {
        let mut catalogs = Catalogs::new("/usr/bin/hgs", None);
        let stamp = catalogs.stamp("studio", "/home/example/repo");
        let data = catalogs
            .accept(
                "studio",
                "/home/example/repo",
                &stamp,
                ok(catalog(1000.0)),
                1000.0,
            )
            .unwrap();
        assert_eq!(data["received_at"], json!(1000.0));
        assert!(catalogs
            .fresh("studio", "/home/example/repo", 1029.0)
            .is_some());
        assert!(catalogs
            .fresh("studio", "/home/example/repo", 1030.0)
            .is_none());
        // Each worktree root gets the same catalog.
        let sibling = catalogs
            .fresh("studio", "/home/example/repo-work-1", 1001.0)
            .unwrap();
        assert_eq!(sibling["selected_root"], "/home/example/repo-work-1");
        // Another computer's identical path is unrelated.
        assert!(catalogs.snapshot("", "/home/example/repo").is_none());
        // A read under way when the repository changes fails.
        let stamp = catalogs.stamp("studio", "/home/example/repo");
        catalogs.invalidate("studio", "/home/example/repo/.git");
        assert!(
            catalogs
                .fresh("studio", "/home/example/repo", 1001.0)
                .is_none(),
            "stale"
        );
        assert_eq!(
            catalogs.accept(
                "studio",
                "/home/example/repo",
                &stamp,
                ok(catalog(1002.0)),
                1002.0
            ),
            Err("Folder changed. Refresh its catalog.".into())
        );
        let stamp = catalogs.stamp("studio", "/home/example/repo");
        assert_eq!(
            catalogs.accept(
                "studio",
                "/home/example/repo",
                &stamp,
                ok(json!({"state": "ok"})),
                1003.0
            ),
            Err("Invalid worktree catalog response".into())
        );
        // The saved snapshots come back after a restart, keyed by executable.
        let restored = Catalogs::new("/usr/bin/hgs", Some(catalogs.saved()));
        assert!(restored.snapshot("studio", "/home/example/repo").is_some());
        assert!(Catalogs::new("/opt/hgs", Some(catalogs.saved()))
            .snapshot("studio", "/home/example/repo")
            .is_none());
    }

    #[test]
    fn snapshots_are_bounded_oldest_first() {
        let mut catalogs = Catalogs::new("hgs", None);
        for index in 0..70 {
            let path = format!("/home/example/r{index}");
            let stamp = catalogs.stamp("", &path);
            let mut data =
                json!({"state": "unavailable", "worktrees": [], "sampled_at": f64::from(index)});
            data["path"] = json!(path);
            catalogs
                .accept("", &path, &stamp, ok(data), 2000.0)
                .unwrap();
        }
        let saved = catalogs.saved();
        assert_eq!(saved.as_object().unwrap().len(), SNAPSHOTS);
        assert!(catalogs.snapshot("", "/home/example/r5").is_none());
        assert!(catalogs.snapshot("", "/home/example/r6").is_some());
    }

    #[test]
    fn creation_and_receipts_follow_qt() {
        let reply = json!({"request_id": "req-1", "status": "created", "branch": "work-1", "common_dir": "/r/.git", "path": "/r-work-1"});
        assert!(created(
            ok(reply.clone()),
            "req-1",
            "work-1",
            "/r/.git",
            false,
            "/r-work-1"
        )
        .is_ok());
        let unconfirmed = "Creation could not be confirmed. Refresh the catalog and check the destination before retrying.";
        for (key, value) in [
            ("request_id", json!("req-2")),
            ("branch", json!("other")),
            ("path", json!("relative")),
        ] {
            let mut changed = reply.clone();
            changed[key] = value;
            assert_eq!(
                created(
                    ok(changed),
                    "req-1",
                    "work-1",
                    "/r/.git",
                    false,
                    "/r-work-1"
                ),
                Err(unconfirmed.into()),
                "{key}"
            );
        }
        let pending = json!({"request_id": "req-1", "status": "running"});
        assert_eq!(
            created(ok(pending), "req-1", "work-1", "/r/.git", true, "/r-work-1"),
            Err("Worktree creation is not confirmed. Check its original result.".into())
        );
        let failed = Exit::Code {
            code: 1,
            stdout: vec![],
            stderr: "branch exists".into(),
        };
        assert_eq!(
            created(failed, "req-1", "work-1", "/r/.git", false, "/r-work-1"),
            Err(
                "branch exists\nCheck /r-work-1 before retrying. Any created worktree is kept."
                    .into()
            )
        );
        assert!(receipt(
            ok(json!({"schema": 1, "request_id": "req-1", "status": "removed"})),
            "req-1"
        )
        .is_ok());
        assert_eq!(
            receipt(
                ok(json!({"schema": 2, "request_id": "req-1", "status": "x"})),
                "req-1"
            ),
            Err("Invalid worktree receipt.".into())
        );
        assert_eq!(
            cleanup_error("", "usage: hgs worktrees --path P"),
            "Update Zerus on this computer to clean up worktrees."
        );
        assert_eq!(
            cleanup_error("studio", "usage: hgs worktrees --path P"),
            "Update Zerus on studio to clean up worktrees."
        );
        assert_eq!(cleanup_error("studio", "denied"), "denied");
        let verified_ok = verified(ok(json!({"worktrees": [], "state": "ok"})));
        assert!(verified_ok.is_ok());
        assert_eq!(
            verified(ok(json!({"worktrees": {}}))),
            Err("Invalid verified worktree catalog.".into())
        );
    }
}
