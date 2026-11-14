use super::*;

/// Explicit installer mode changes use the same agent refusal as native
/// policy requests. This proves a caller boundary, never a service grant.
pub(in crate::desktop) fn caller(scope: &Scope) -> Result<&'static str> {
    Ok(capture(scope)?.0)
}

/// Routing evidence from the admitted native caller, never input authority.
pub(in crate::desktop) fn observation(scope: &Scope) -> Result<(&'static str, Value)> {
    let (source, process) = capture(scope)?;
    let (machine, boot) = origin::native()?;
    ensure!(process.current(), "installer consumer changed");
    Ok((
        source,
        serde_json::json!({"machine":machine,"boot":boot,
        "pid":process.pid,"birth":process.identity.birth}),
    ))
}

#[path = "registration_origin.rs"]
mod origin;

fn capture(scope: &Scope) -> Result<(&'static str, Process)> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "use the desktop user, without sudo"
    );
    // A resident DeepSeek host's tool call is an agent's even without session
    // markers it can prove (zerus-ppd26).
    ensure!(
        ["HGS_SESSION", "HGS_SESSION_ID", "HGS_RUN_ID"]
            .iter()
            .all(|name| std::env::var_os(name).is_none())
            && !crate::state::session_graph::hosted_agent_call(),
        "agent session cannot change desktop mode; use Settings or a terminal outside Zerus"
    );
    // Nor any command under a recorded DeepSeek host, which the process tree
    // and the host records decide (zerus-ppd26.15).
    if let Some(refusal) = crate::state::dsh::host_refusal() {
        anyhow::bail!("{refusal} Change desktop mode from Settings or a terminal outside Zerus.");
    }
    let until = Instant::now() + WAIT;
    #[cfg(target_os = "macos")]
    let owned = macos_ancestry::inspect(std::process::id(), until)?;
    #[cfg(not(target_os = "macos"))]
    check_ancestry(std::process::id(), until)?;
    if let Ok(tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open("/dev/tty")
    {
        // SAFETY: the owned descriptor stays open through both queries.
        if unsafe { libc::isatty(tty.as_raw_fd()) } == 1
            && unsafe { libc::tcgetpgrp(tty.as_raw_fd()) } == unsafe { libc::getpgrp() }
        {
            // The actual foreground process-group leader is the admitted
            // terminal consumer. Never choose a renderer-supplied PID.
            let group = unsafe { libc::tcgetpgrp(tty.as_raw_fd()) };
            let process = Process::read(u32::try_from(group)?)?;
            // A reparented caller can retain this group after its leader has
            // left the caller's ancestry. Check the actual consumer as well.
            // A platform-withheld environment remains no positive evidence.
            ensure!(
                !agent_image(&process.executable) && !agent_environment(process.pid)?,
                "agent-descended frontend request refused"
            );
            ensure!(process.current(), "installer terminal consumer changed");
            return Ok(("interactive_terminal", process));
        }
    }
    #[cfg(target_os = "macos")]
    for process in owned.iter().skip(1) {
        ensure!(
            Instant::now() < until && process.current(),
            "installer caller changed"
        );
        if scope.selected_frontend(Path::new(&process.executable)) {
            ensure!(process.current(), "installer frontend changed");
            return Ok(("installed_frontend", process.clone()));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let original = Process::read(std::process::id())?;
        ensure!(original.current(), "installer caller changed");
        let mut pid = original.identity.parent;
        let mut seen = BTreeSet::new();
        while pid != 0 {
            ensure!(
                seen.len() < ANCESTORS && Instant::now() < until && seen.insert(pid),
                "installer caller ancestry unavailable"
            );
            if !same_user(pid)? {
                break;
            }
            let process = Process::read(pid)?;
            ensure!(process.current(), "installer caller changed");
            if scope.selected_frontend(Path::new(&process.executable)) {
                return Ok(("installed_frontend", process));
            }
            pid = process.identity.parent;
        }
    }
    anyhow::bail!("desktop mode changes require Settings or a foreground terminal outside Zerus")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: &str = "desktop::ui_authority::registration::tests::hosted_capture_probe";

    #[test]
    fn desktop_mode_capture_refuses_a_hosted_call_without_session_markers() {
        let temp = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", PROBE, "--ignored", "--nocapture"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", temp.path())
            .env("HGS_CONFIG_DIR", temp.path().join("config"))
            .env("HGS_STATE_DIR", temp.path().join("state"))
            .env("HGS_AGENT", "dsh")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "isolated registration capture; launched only with private configuration"]
    fn hosted_capture_probe() {
        assert_eq!(std::env::var("HGS_AGENT").as_deref(), Ok("dsh"));
        for marker in ["HGS_SESSION", "HGS_RUN_ID", "HGS_SESSION_ID"] {
            assert!(std::env::var_os(marker).is_none());
        }
        let config = crate::config::Config::load().unwrap();
        let location = Location {
            root: config.state.join("desktop"),
            configuration: "a".repeat(64),
        };
        let scope = Scope::new(&config, &location);
        let error = capture(&scope)
            .err()
            .expect("a hosted call cannot capture desktop mode authority");
        assert_eq!(
            error.to_string(),
            "agent session cannot change desktop mode; use Settings or a terminal outside Zerus"
        );
        assert!(
            !location.root.exists(),
            "refusal must precede registration effects"
        );
    }
}
