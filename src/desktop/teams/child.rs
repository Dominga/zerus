//! The Teams original is this computer's own hgs, with its own close-only
//! custody descriptor. No agent identity or presentation parent is inherited.
use crate::{
    config::Config,
    desktop::original_child::{self, Codes, Input, Output, Spec, Stdin},
};
pub(super) use original_child::Ended;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};
use zerus_client_core::run::custody::InheritedLock;
pub(super) struct Original<'a> {
    pub id: &'a str,
    pub plan: &'a super::ports::Plan,
    pub bound: bool,
}
const CUSTODY: &str = "HGS_TEAMS_CUSTODY_FD";
const CODES: Codes = Codes {
    unobserved: "team_child_unobserved",
    unreadable: "team_output_unreadable",
    signalled: "team_child_signalled",
    timed_out: "team_child_timed_out",
    retained: "team_output_retained",
};

pub(crate) fn adopt_custody() -> Result<(), String> {
    original_child::adopt(
        CUSTODY,
        "Invalid Teams custody",
        "Teams custody is unavailable",
    )
}
#[cfg(test)]
type FixtureRun = Box<dyn FnMut(&Config, &[String], Option<&[u8]>) -> Ended>;
#[cfg(test)]
std::thread_local! {
    pub(super) static FIXTURE_RUN: std::cell::RefCell<Option<FixtureRun>> = const { std::cell::RefCell::new(None) };
    pub(super) static FIXTURE_EXIT: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
    /// The argv each fixture child was given, so a test can run what the
    /// service spawns through the real CLI.
    pub(super) static FIXTURE_ARGS: std::cell::RefCell<Vec<Vec<String>>> = const { std::cell::RefCell::new(Vec::new()) };
}
pub(super) fn run(
    config: &Config,
    args: &[String],
    input: Option<&[u8]>,
    original: Option<Original<'_>>,
    stderr: bool,
    deadline: Instant,
    stop: &AtomicBool,
) -> Ended {
    let id = original.as_ref().map(|o| o.id);
    run_guarded(config, args, input, id, stderr, deadline, stop, || {
        if let Some(o) = &original {
            admission(config, o.plan, o.bound)?;
        }
        Ok(())
    })
}
/// A Teams effect has its own stored-effect admission, with the same child,
/// configuration, process group, bounded drains and close-only custody.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_guarded<T>(
    config: &Config,
    args: &[String],
    input: Option<&[u8]>,
    original_id: Option<&str>,
    stderr: bool,
    deadline: Instant,
    stop: &AtomicBool,
    admit: impl FnOnce() -> super::ports::Result<T>,
) -> Ended {
    #[cfg(test)]
    if FIXTURE_RUN.with(|slot| slot.borrow().is_some()) {
        let mut command = std::process::Command::new("fixture");
        return match configure(config, &mut command, stop, admit) {
            Ok(_guard) => FIXTURE_RUN.with(|slot| {
                slot.borrow_mut().as_mut().expect("fixture transport")(config, args, input)
            }),
            Err(_) => Ended::NotStarted,
        };
    }
    #[cfg(test)]
    if let Some(output) = FIXTURE_EXIT.with(|slot| slot.borrow_mut().take()) {
        FIXTURE_ARGS.with(|slot| slot.borrow_mut().push(args.to_vec()));
        let mut command = std::process::Command::new("fixture");
        return match configure(config, &mut command, stop, admit) {
            Ok(guard) => {
                if let Some(hook) =
                    original_child::AFTER_FINAL_CHECK.with(|s| s.borrow_mut().take())
                {
                    hook();
                }
                drop(guard);
                if let Some(hook) = original_child::AFTER_SPAWN.with(|s| s.borrow_mut().take()) {
                    hook();
                }
                Ended::Exited { code: 0, output }
            }
            Err(_) => Ended::NotStarted,
        };
    }

    let Ok(program) = crate::self_exec::current() else {
        return Ended::NotStarted;
    };
    let spec = spec(input, stderr);
    let check = |command: &mut std::process::Command| configure(config, command, stop, admit);
    match original_id {
        None => original_child::run_unheld(&program, args, spec, deadline, |command| {
            check(command).map(drop)
        }),
        Some(id) => {
            let Some((held, path, identity)) = custody(config, id) else {
                return Ended::NotStarted;
            };
            let result = original_child::run(&program, args, spec, &held, deadline, check);
            if !matches!(result, Ended::Unknown(_)) {
                remove_custody(&path, identity);
            }
            result
        }
    }
}
fn spec(input: Option<&[u8]>, stderr: bool) -> Spec<'_> {
    let input = input.map_or(Input::Closed, |bytes| Input::Frozen {
        stdin: Stdin::Bytes(bytes),
        unwritten: "team_input_unwritten",
    });
    let output = if stderr {
        Output::StderrPrefix(8192)
    } else {
        Output::Stdout {
            bytes: 256 << 10,
            too_large: "team_output_too_large",
        }
    };
    Spec {
        custody_env: CUSTODY,
        input,
        output,
        codes: CODES,
        retain: None,
    }
}
fn configure<T>(
    config: &Config,
    command: &mut std::process::Command,
    stop: &AtomicBool,
    admit: impl FnOnce() -> super::ports::Result<T>,
) -> std::io::Result<T> {
    if stop.load(Ordering::Acquire) || !crate::teams::enabled(&config.state) {
        return Err(std::io::ErrorKind::Interrupted.into());
    }
    for var in [
        "HGS_SESSION",
        "HGS_SESSION_ID",
        "HGS_RUN_ID",
        "HGS_CONVERSATION_ID",
        "HGS_LAUNCH_ID",
        "HGS_LAUNCH_SESSION_ID",
        "HGS_ACCOUNT_ID",
        "HGS_PARENT",
        "TMUX",
        "TMUX_PANE",
    ] {
        command.env_remove(var);
    }
    command
        .env("HGS_CONFIG_DIR", &config.dir)
        .env("HGS_STATE_DIR", &config.state);
    let guard = admit().map_err(|_| std::io::Error::from(std::io::ErrorKind::PermissionDenied))?;
    // The caller reads its in-process fleet facts inside this admission.
    // Recheck stop and the Teams switch last while that fence is still held;
    // child-based occupancy reads belong before admission, outside spawn_turn.
    if stop.load(Ordering::Acquire) || !crate::teams::enabled(&config.state) {
        return Err(std::io::ErrorKind::Interrupted.into());
    }
    Ok(guard)
}

pub(super) fn custody(
    config: &Config,
    id: &str,
) -> Option<(Arc<InheritedLock>, PathBuf, (u64, u64))> {
    if !uuid::Uuid::parse_str(id).is_ok_and(|u| !u.is_nil() && u.to_string() == id) {
        return None;
    }
    let dir = config.state.join("teams/custody");
    crate::platform::private_dir(&dir).ok()?;
    if !crate::platform::verify_private(&dir, crate::platform::Private::Directory).ok()? {
        return None;
    }
    let path = dir.join(format!("{id}.lock"));
    let mut file = crate::platform::no_follow(crate::platform::private_open(
        OpenOptions::new().create_new(true).read(true).write(true),
    ))
    .open(&path)
    .ok()?;
    if !crate::platform::verify_private_file(&file).ok()? {
        return None;
    }
    let marker = serde_json::json!({"kind":"teams_original","id":id,"configuration":config.dir,"state":config.state});
    file.write_all(marker.to_string().as_bytes()).ok()?;
    file.sync_all().ok()?;
    std::fs::File::open(&dir).ok()?.sync_all().ok()?;
    let metadata = file.metadata().ok()?;
    let identity = (metadata.dev(), metadata.ino());
    Some((InheritedLock::try_lock(file).ok()?, path, identity))
}

pub(super) fn admission(
    config: &Config,
    plan: &super::ports::Plan,
    bound: bool,
) -> super::ports::Result<()> {
    use super::launch::invalid;
    use zerus_team_plan::model::{MemberState, TeamStatus};
    let store = crate::teams::store::Store::open(&config.state)?;
    let team = store
        .team(&plan.team)?
        .ok_or_else(|| invalid("Team disappeared"))?;
    let member = store
        .member(&plan.team, &plan.member)?
        .ok_or_else(|| invalid("Member disappeared"))?;
    let session = member.session.as_ref().is_some_and(|key| {
        key.machine == plan.machine && key.session_id == format!("launch_id:{}", plan.launch_id)
    });
    if team.status != TeamStatus::Running
        || team.home != plan.machine
        || team.project != plan.project
        || team.owner != plan.person
        || member.role != plan.role
        || member.agent != plan.agent
        || if bound {
            member.state != MemberState::Active || !session
        } else {
            member.state != MemberState::NotCreated || member.session.is_some()
        }
    {
        return Err(invalid("Member launch admission changed"));
    }
    Ok(())
}

pub(super) fn remove_custody(path: &Path, identity: (u64, u64)) {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && (m.dev(), m.ino()) == identity)
    {
        let _ = std::fs::remove_file(path);
    }
}
/// Best-effort, bounded collection of only our marked, private, unlocked files.
/// Lock release is a resource fact; it never settles or retries an unknown row.
pub(super) fn prune_custody(config: &Config) {
    let dir = config.state.join("teams/custody");
    if !matches!(
        crate::platform::verify_private(&dir, crate::platform::Private::Directory),
        Ok(true)
    ) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.take(128).flatten() {
        let path = entry.path();
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().is_none_or(|s| s != "lock")
            || !uuid::Uuid::parse_str(id).is_ok_and(|u| u.to_string() == id && !u.is_nil())
        {
            continue;
        }
        let Ok(file) =
            crate::platform::no_follow(OpenOptions::new().read(true).write(true)).open(&path)
        else {
            continue;
        };
        if !matches!(crate::platform::verify_private_file(&file), Ok(true)) {
            continue;
        }
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        if metadata.len() > 4096 {
            continue;
        }
        let mut raw = Vec::new();
        if (&file).take(4097).read_to_end(&mut raw).is_err() {
            continue;
        }
        let Ok(marker) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            continue;
        };
        if marker["kind"] != "teams_original"
            || marker["id"] != id
            || marker["configuration"] != serde_json::json!(config.dir)
            || marker["state"] != serde_json::json!(config.state)
        {
            continue;
        }
        if let Ok(_held) = InheritedLock::try_lock(file) {
            remove_custody(&path, (metadata.dev(), metadata.ino()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_pruning_keeps_live_custody_and_unknown_files_but_removes_released_originals() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            home: String::new(),
            dir: temp.path().join("config"),
            state: temp.path().join("state"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        let id = uuid::Uuid::new_v4().to_string();
        let (held, path, _) = custody(&config, &id).unwrap();
        let other = path
            .parent()
            .unwrap()
            .join(format!("{}.lock", uuid::Uuid::new_v4()));
        std::fs::write(&other, b"unknown local data").unwrap();
        prune_custody(&config);
        assert!(path.exists());
        drop(held);
        // A concurrent fixture may be between fork and exec, before its
        // CLOEXEC descriptor closes. Pruning conservatively waits for release.
        for _ in 0..100 {
            prune_custody(&config);
            if !path.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!path.exists());
        assert!(other.exists());
    }
}
