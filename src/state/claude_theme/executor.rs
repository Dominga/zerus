//! A fixed route inside the resident supervisor. It owns no ambient mode and
//! cannot alter the ordinary backend, Effort or Settings after it returns.
use super::*;
use super::{
    helper::Source,
    owner::{Acquisition, Binding, Scope},
};
use backend::{Capture, InputClaim, InputState, Key, PaneInfo, TerminalBackend};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    process::{Child, ExitStatus},
    sync::Arc,
    time::{Duration, Instant},
};

/// Created before either launch. Its spent state and absolute observation cap
/// survive the first->base fallback; a new PID never creates a retry.
pub(in crate::state) struct Ticket {
    session: String,
    run: String,
    pane: String,
    supervisor_birth: String,
    request: String,
    spent: bool,
    started: Option<Instant>,
}
impl Ticket {
    pub(in crate::state) fn new(agent: &str, session: &str, run: &str) -> Option<Self> {
        if agent != "claude" || std::env::var("HGS_AGENT").ok().as_deref() != Some("claude") {
            return None;
        }
        Some(Self {
            session: session.into(),
            run: run.into(),
            pane: backend::current().own_pane().ok()?,
            supervisor_birth: crate::platform::process_identity(std::process::id())?.birth,
            request: uuid::Uuid::new_v4().to_string(),
            spent: false,
            started: None,
        })
    }
    fn binding(&self, spawned: &supervisor::Spawned) -> Option<Binding> {
        let r = &spawned.record;
        if self.spent
            || !eligible(r)
            || r["pid"].as_u64() != Some(u64::from(spawned.child.id()))
            || string(r, "session_id") != self.session
            || string(r, "run_id") != self.run
            || string(r, "pane") != self.pane
            || string(r, "process_start").is_empty()
            || r["supervisor"]["pid"].as_u64() != Some(u64::from(std::process::id()))
            || string(&r["supervisor"], "start").is_empty()
        {
            return None;
        }
        Some(Binding {
            record: r.clone(),
            process: spawned.identity.clone()?,
            supervisor_birth: self.supervisor_birth.clone(),
        })
    }
}

struct Native(Arc<Scope>);
impl TerminalBackend for Native {
    fn input_state(&self, claim: &InputClaim) -> Result<InputState> {
        self.0.actor()?;
        let record = &self.0.record;
        if claim.pane != string(record, "pane")
            || claim.name != terminal_name(record)
            || claim.run_id != string(record, "run_id")
            || claim.agent != "claude"
            || claim.pid != record["pid"].as_u64()
        {
            return Err("Claude appearance input claim changed".into());
        }
        backend::input_state_using(
            claim,
            |fields| {
                if fields != helper::INPUT_FIELDS {
                    return Err("Unreviewed appearance pane fields".into());
                }
                let output = helper::run(&self.0, helper::Call::Pane(claim.pane.into()))?;
                Ok(PaneInfo::new(
                    String::from_utf8(output.stdout).map_err(|e| e.to_string())?,
                ))
            },
            |_| {
                let pid = u32::try_from(claim.pid.ok_or("Missing Claude appearance PID")?)
                    .map_err(|e| e.to_string())?;
                helper::run(&self.0, helper::Call::Process(pid))
            },
        )
    }
    fn capture(&self, pane: &str, mode: Capture) -> Result<Vec<u8>> {
        self.0.actor()?;
        if pane != string(&self.0.record, "pane") || !matches!(mode, Capture::Plain) {
            return Err("Unreviewed Claude appearance capture refused".into());
        }
        Ok(helper::run(&self.0, helper::Call::Capture(pane.into()))?.stdout)
    }
    fn send(&self, pane: &str, keys: &[Key]) -> Result<()> {
        self.0.actor()?;
        if pane != string(&self.0.record, "pane") || !matches!(keys, [Key::Named("Enter")]) {
            return Err("Only one original appearance Enter is supported".into());
        }
        helper::boundary(self.0.until())?;
        helper::run(&self.0, helper::Call::Enter(pane.into())).map(|_| ())
    }
}

fn alive(child: &mut Child, record: &Value) -> Result<()> {
    if supervisor::shutdown_requested() {
        return Err("Claude appearance setup stopped".into());
    }
    if child.id()
        != record["pid"]
            .as_u64()
            .ok_or("Missing original Claude child")? as u32
        || child.try_wait().map_err(|e| e.to_string())?.is_some()
    {
        return Err("Original Claude startup child exited".into());
    }
    Ok(())
}

fn screen(native: &Native) -> Result<String> {
    String::from_utf8(native.capture(string(&native.0.record, "pane"), Capture::Plain)?)
        .map_err(|e| e.to_string())
}

fn observed(native: &Native, child: &mut Child, expected: &parser::Panel) -> Result<()> {
    // Every Child probe follows the preceding helper's actual wait/EOF/joins.
    alive(child, &native.0.record)?;
    native.0.check()?;
    let output = helper::run(
        &native.0,
        helper::Call::Layout(terminal_name(&native.0.record).into()),
    )?;
    let expected_layout = format!(
        "{}\t0\t{}",
        string(&native.0.record, "pane"),
        string(&native.0.record, "run_id")
    );
    if String::from_utf8_lossy(&output.stdout).trim_end_matches('\n') != expected_layout {
        return Err("Claude startup terminal no longer has its sole original pane".into());
    }
    input::checked_terminal_on(&native.0.record, false, native)?;
    if parser::parse(&screen(native)?).as_ref() != Some(expected) {
        return Err("Claude appearance changed; nothing further was submitted".into());
    }
    Ok(())
}

fn confirm(
    scope: &Arc<Scope>,
    child: &mut Child,
    ticket: &mut Ticket,
    panel: &parser::Panel,
) -> Result<()> {
    let native = Native(scope.clone());
    observed(&native, child, panel)?;
    let first = Instant::now();
    while first.elapsed() < kimi_tui_choice::CLAUDE_CONFIRMATION_GUARD {
        std::thread::sleep(Duration::from_millis(20));
        observed(&native, child, panel)?;
    }
    helper::boundary(scope.until())?;
    alive(child, &scope.record)?;
    ticket.spent = true; // A failed/unknown durable publication never authorizes replay.
    scope.reserve(&ticket.request, panel)?;
    observed(&native, child, panel)?;
    alive(child, &scope.record)?;
    helper::boundary(scope.until())?; // shutdown after guard and immediately before Enter
    scope.permit()?;
    native.send(string(&scope.record, "pane"), &[Key::Named("Enter")])?;
    let ack = (Instant::now() + Duration::from_secs(2)).min(scope.until());
    while Instant::now() < ack {
        std::thread::sleep(Duration::from_millis(40));
        alive(child, &scope.record)?;
        scope.check()?;
        let screen = screen(&native)?;
        // A blank/partial redraw is not positive advancement. A complete next
        // manual chooser is recognized without answering any of its options.
        if parser::parse(&screen).is_none() && startup_choice::recognized(&screen) {
            scope.outcome("advanced", "next manual startup chooser")?;
            return Ok(());
        }
    }
    Err("Claude appearance advancement is unconfirmed; automatic Enter will not repeat".into())
}

fn capture(binding: Binding, until: Instant) -> Result<String> {
    let pane = string(&binding.record, "pane").to_owned();
    let (source, done) = owner::Observation::new(binding, until)?;
    let result = catch_unwind(AssertUnwindSafe(|| {
        helper::run(&source, helper::Call::Capture(pane))
    }));
    drop(source);
    owner::completed(done);
    match result {
        Ok(result) => String::from_utf8(result?.stdout).map_err(|e| e.to_string()),
        Err(_) => Err("Claude appearance observation failed; original resources settled".into()),
    }
}

pub(in crate::state) fn wait(
    spawned: &mut supervisor::Spawned,
    ticket: &mut Ticket,
) -> Result<ExitStatus> {
    let Some(binding) = ticket.binding(spawned) else {
        return supervisor::wait(&mut spawned.child);
    };
    let started = *ticket.started.get_or_insert_with(Instant::now);
    let cap = started + Duration::from_secs(60);
    loop {
        if ticket.spent
            || helper::boundary(cap).is_err()
            || alive(&mut spawned.child, &binding.record).is_err()
        {
            break;
        }
        let until = (Instant::now() + Duration::from_secs(3)).min(cap);
        let screen = match capture(binding.clone(), until) {
            Ok(screen) => screen,
            Err(_) => break,
        };
        let panel = parser::parse(&screen);
        if let Some(panel) = panel {
            if panel.selected_default().is_none() {
                break;
            }
            match Scope::acquire(
                binding.clone(),
                (Instant::now() + Duration::from_secs(3)).min(cap),
            ) {
                Ok(Acquisition::Busy) => {}
                Ok(Acquisition::Spent) => {
                    ticket.spent = true;
                    break;
                }
                Err(_) => break,
                Ok(Acquisition::Ready(scope, done)) => {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        confirm(&scope, &mut spawned.child, ticket, &panel)
                    }));
                    if ticket.spent {
                        let detail = match &result {
                            Ok(Err(error)) => error.as_str(),
                            Err(_) => "appearance executor unwound",
                            _ => "",
                        };
                        if !detail.is_empty() {
                            let status = if scope
                                .may_have_sent
                                .load(std::sync::atomic::Ordering::Acquire)
                            {
                                "unknown"
                            } else {
                                "abandoned"
                            };
                            let _ = scope.outcome(status, detail);
                        }
                    }
                    scope.end();
                    drop(scope);
                    // This sentinel is the LAST field, after the original
                    // locks/proofs, and cannot fire before physical helper drain.
                    owner::completed(done);
                    // A refusal/panic ends this observer; no generic mode or
                    // sticky Effort flag can escape into finish/archive/fallback.
                    break;
                }
            }
        } else if startup_choice::recognized(&screen) {
            break;
        }
        std::thread::sleep(if started.elapsed() < Duration::from_secs(10) {
            Duration::from_millis(250)
        } else {
            Duration::from_millis(1500)
        });
    }
    supervisor::wait(&mut spawned.child)
}
