//! Original direct helper custody. These closed forms are selected only by
//! the Theme executor; no wire data chooses a program, argv or effect class.
use super::*;
use std::{
    process::{Command, Output, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use zerus_client_core::run::writer_custody::Admission;

pub(super) const INPUT_FIELDS: [backend::PaneField; 9] = [
    backend::PaneField::Tty,
    backend::PaneField::CursorX,
    backend::PaneField::CursorY,
    backend::PaneField::InMode,
    backend::PaneField::CurrentCommand,
    backend::PaneField::Dead,
    backend::PaneField::RunMarker,
    backend::PaneField::SessionName,
    backend::PaneField::BracketedPaste,
];

pub(super) enum Call {
    Capture(String),
    Pane(String),
    Layout(String),
    Process(u32),
    Enter(String),
}
impl Call {
    fn command(&self) -> Command {
        let mut command = Command::new(if matches!(self, Self::Process(_)) {
            "ps"
        } else {
            "tmux"
        });
        match self {
            Self::Capture(pane) => {
                command.args(["capture-pane", "-p", "-t", pane]);
            }
            Self::Pane(pane) => {
                command.args(backend::pane_info_args(
                    backend::Target::Pane(pane),
                    &INPUT_FIELDS,
                ));
            }
            Self::Layout(terminal) => {
                command.args([
                    "list-panes",
                    "-s",
                    "-t",
                    &format!("={terminal}"),
                    "-F",
                    "#{pane_id}\t#{pane_dead}\t#{@hgs_run}",
                ]);
            }
            Self::Process(pid) => {
                command.args(["-p", &pid.to_string(), "-o", "tty=,pgid=,tpgid=,comm="]);
            }
            Self::Enter(pane) => {
                command.args(["send-keys", "-t", pane, "Enter"]);
            }
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

pub(super) trait Source: Send + Sync + 'static {
    /// Original objects and native identity, never a nested native callback.
    fn check(&self) -> Result<()>;
    fn until(&self) -> Instant;
    /// The fixed owner checks its actor and exact allowed target here. Enter
    /// consumes its single prepared permit and marks possible-send BEFORE spawn.
    fn offer(&self, call: &Call) -> Result<()>;
}

pub(super) fn boundary(until: Instant) -> Result<()> {
    if supervisor::shutdown_requested() || Instant::now() >= until {
        Err("Claude appearance setup stopped or expired".into())
    } else {
        Ok(())
    }
}

pub(super) fn run<S: Source>(source: &Arc<S>, call: Call) -> Result<Output> {
    source.check()?;
    let until = source.until();
    let charge: Arc<dyn Send + Sync> = source.clone();
    let admission = Admission::reserve(charge, until)
        .map_err(|error| format!("Claude appearance helper admission refused: {error:?}"))?;
    let mut command = call.command();
    let child = {
        let _turn = crate::platform::spawn_turn();
        source.check()?;
        source.offer(&call)?;
        boundary(until)?;
        command.spawn().map_err(|error| error.to_string())?
    };
    // No post-spawn check, refusal or unwind can lose this exact Child/pipes.
    let mut exchange = admission.attach(child);
    let _ = exchange.close_empty_input(|_, original| original == until && source.check().is_ok());
    loop {
        if let Some(report) = exchange.poll() {
            source.check()?;
            if report.failure.is_some() || !report.input_written {
                return Err(format!(
                    "Claude appearance helper did not settle successfully: {:?}",
                    report.failure
                ));
            }
            let output = Output {
                status: report.exit,
                stdout: report.stdout,
                stderr: report.stderr,
            };
            if !output.status.success() {
                return Err(format!(
                    "Claude appearance helper refused: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            return Ok(output);
        }
        if let Err(error) = boundary(until) {
            exchange.cancel();
            // Its retirement retains this SAME source, Child and all streams.
            // The caller waits source completion before waiting on the agent.
            return Err(error);
        }
        std::thread::sleep(
            Duration::from_millis(2).min(until.saturating_duration_since(Instant::now())),
        );
    }
}
