//! Explicit same-mode original-instance refresh. Existing UUIDs only read the
//! durable result. No mode toggle, definition UPDATE, reinstall or revoke lane.
use super::*;
use policy::mode::refresh::{self as journal, Ready, Scope, Target};

const ANCHOR_BYTES: usize = 1024;
fn anchor_path(config: &Config, location: &Location) -> std::path::PathBuf {
    config
        .dir
        .join(format!("windows-refresh-{}.json", location.configuration))
}
fn anchor(config: &Config, location: &Location, plan: &Plan) -> Result<Option<String>> {
    match crate::desktop::handoff::read_private(&anchor_path(config, location), ANCHOR_BYTES) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)?;
            let id = value["request"].as_str().unwrap_or("");
            ensure!(
                value["schema"] == 1
                    && value["configuration"] == location.configuration
                    && value["sid"] == plan.sid
                    && value.as_object().is_some_and(|o| o.len() == 4)
                    && uuid::Uuid::parse_str(id).is_ok_and(|u| !u.is_nil() && u.to_string() == id),
                "Original refresh anchor is unavailable; keep it and the mode journal."
            );
            Ok(Some(id.into()))
        }
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
pub(super) fn lost(
    config: &Config,
    location: &Location,
    plan: &Plan,
    record: &Record,
) -> Result<Option<String>> {
    Ok(record.refresh_lost(anchor(config, location, plan)?.as_deref()))
}
fn write_anchor(
    config: &Config,
    location: &Location,
    plan: &Plan,
    request: &str,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    ensure!(anchor_layout_separate(config, location),
        "Refresh needs the config folder outside the desktop state loss domain; no effect was offered.");
    check()?;
    let bytes = serde_json::to_vec(&json!({"schema":1,"configuration":location.configuration,
        "sid":plan.sid,"request":request}))?;
    let mut file = tempfile::NamedTempFile::new_in(&config.dir)?;
    platform::make_private(file.as_file())?;
    ensure!(
        platform::verify_private_file(file.as_file())?,
        "Refresh anchor is not private."
    );
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    check()?;
    platform::persist(file, anchor_path(config, location)).map_err(|e| e.error)?;
    platform::sync_dir(&config.dir)?;
    check()
}
fn plan_for<'a>(mode: Mode, passive: &'a Plan, background: &'a Plan) -> &'a Plan {
    if mode == Mode::Passive {
        passive
    } else {
        background
    }
}
fn current_target(
    location: &Location,
    plan: &Plan,
    running: &instances::Running,
    grant: &ownership::RegistrationGrant,
    record: &Record,
) -> Result<Target> {
    running.check()?;
    grant.check(location)?;
    let (journal, owner_epoch) = grant.incarnation();
    let (original_image, replacement_image) = running.refresh_images()?;
    use sha2::{Digest, Sha256};
    let selection = format!("{:x}", Sha256::digest(record.encode(plan)?));
    let target = Target {
        configuration: location.configuration.clone(),
        selection,
        mode: if plan.argv.last().is_some_and(|v| v == "--background") {
            Mode::Background
        } else {
            Mode::Passive
        },
        original: running.observation(),
        original_generation: running.scope().1,
        journal,
        owner_epoch,
        grant_binding: grant.refresh_binding(location)?,
        original_image,
        replacement_image,
    };
    target.validate()?;
    Ok(target)
}
fn marker(config: &Config, binary: &str) -> Result<()> {
    let invocation = zerus_client_core::actions::Invocation {
        args: vec!["--help".into()],
        stdin: None,
        timeout: Duration::from_secs(2),
        request_id: None,
    };
    let env = environment(config, binary)?;
    let refs = env
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect::<Vec<_>>();
    let result =
        zerus_client_core::run::run_hidden_limited(Path::new(binary), &invocation, &refs, CAP)
            .map_err(|e| anyhow::anyhow!("Selected refresh helper probe is unavailable: {e:?}"))?;
    let zerus_client_core::actions::Exit::Code {
        code: 0, stdout, ..
    } = result
    else {
        bail!("Selected refresh helper probe failed.");
    };
    ensure!(
        String::from_utf8(stdout)?.lines().any(
            |line| line.trim() == format!("Desktop registration refresh ABI: {}", journal::ABI)
        ),
        "Update the selected CLI before original-instance refresh."
    );
    Ok(())
}
pub(super) fn view(location: &Location, record: &Record, request: &str) -> Value {
    let receipt = record.refreshes.iter().find(|r| r.request == request);
    json!({"abi":MODE_ABI,"refresh_abi":journal::ABI,"schema":1,
        "configuration":location.configuration,"request_id":request,"read_only":true,
        "receipt":receipt,"pending":record.pending.as_ref().filter(|p|p.request_id == request),
        "outcome":receipt.map(|r|r.phase).unwrap_or(policy::mode::review::ChoicePhase::Unknown),
        "changed":false,"manager_changed":false,"authority_changed":false,
        "activation_automatic_blocked":!record.reviewed.is_empty() || record.pending.as_ref().is_some_and(|p|p.review.is_some())})
}
/// Returned only by an original-ID read; no runtime is selected or completed.
pub(super) fn result(config: &Config, location: &Location, request: &str) -> Result<Value> {
    ensure!(
        uuid::Uuid::parse_str(request).is_ok_and(|u| !u.is_nil() && u.to_string() == request),
        "Original refresh UUID required."
    );
    let sid = crate::host::token::user_sid()?;
    let preview = registration::windows_plan(config, location)?;
    let plan = Plan::parse(
        &json!({"schema":1,"operation":"status","plan":preview}),
        &sid,
    )?;
    let (record, loss) = super::journal(&sid, config, location, &plan)?;
    let mut reply = view(location, &record, request);
    if let Some(lost) = loss {
        reply["receipt"] = Value::Null;
        reply["outcome"] = "unknown".into();
        reply["code"] = "mode_journal_lost".into();
        reply["lost_original"] = lost.into();
    }
    Ok(reply)
}

/// Under the retained registration lock. This read creates no original intent,
/// UUID or effect. The later explicit operation must bind the target anew.
pub(super) fn info(runtime: Runtime<'_>) -> Result<Value> {
    let until = Budget::Preflight.deadline(Instant::now());
    runtime.admission.check()?;
    ensure!(
        anchor_layout_separate(runtime.config, runtime.location),
        "Refresh needs the config folder outside the desktop state loss domain."
    );
    let mode = selected(runtime.native, runtime.passive, runtime.background)?;
    let plan = plan_for(mode, runtime.passive, runtime.background);
    let record = load(runtime.location, runtime.passive)?;
    ensure!(
        record.pending.is_none(),
        "Read or review the original pending operation before refresh."
    );
    let grant = ownership::RegistrationGrant::capture(runtime.location)?
        .context("Original refresh requires a current Service Grant; revoked/blocked refresh is unsupported and never Rearms.")?;
    let running = instances::original(
        runtime.native,
        runtime.location,
        plan,
        &wanted(plan)?,
        until,
    )?
    .context("Original running service is unavailable; no replacement is selected.")?;
    let target = current_target(runtime.location, plan, &running, &grant, &record)?;
    let drained = running.retirement_drained(runtime.location, plan, until)?;
    instances::check_original_selection(
        runtime.native,
        plan,
        &wanted(plan)?,
        &running,
        until,
        &mut || {
            runtime.admission.check()?;
            grant.check(runtime.location)
        },
    )?;
    ensure!(
        Instant::now() < until,
        "Refresh observation deadline expired."
    );
    runtime.admission.check()?;
    Ok(json!({"abi":MODE_ABI,"refresh_abi":journal::ABI,"schema":1,
        "configuration":runtime.location.configuration,"target":target,"drained":drained,
        "pending":null,"read_only":true,"changed":false,"manager_changed":false,"authority_changed":false}))
}

fn before_stop(
    native: &Native,
    location: &Location,
    plan: &Plan,
    original: &instances::Running,
    until: Instant,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    // The preceding observer's real EOF can arrive just before the actor
    // releases its connection charge. Poll that observation under THIS Stop
    // deadline; selection/caller/Grant failures still refuse immediately.
    loop {
        check()?;
        instances::check_original_selection(native, plan, &wanted(plan)?, original, until, check)?;
        if original.retirement_drained(location, plan, until)? {
            return check();
        }
        ensure!(
            Instant::now() < until,
            "Original retirement/resource drain deadline expired; no Stop is admitted."
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn effect(runtime: Runtime<'_>, request: &str, target: Target) -> Result<Value> {
    let until = Budget::Preflight.deadline(Instant::now());
    target.validate()?;
    ensure!(
        target.configuration == runtime.location.configuration,
        "Refresh configuration changed."
    );
    let Runtime {
        config,
        location,
        native,
        admission,
        passive,
        background,
        ..
    } = runtime;
    let mut record = load(location, passive)?;
    if record.refreshes.iter().any(|r| r.request == request) {
        return Ok(view(location, &record, request));
    }
    ensure!(
        record.pending.is_none(),
        "An original operation is pending; read its UUID without replay."
    );
    ensure!(
        anchor_layout_separate(config, location),
        "Refresh anchor layout is unavailable."
    );
    admission.check()?;
    ensure!(
        selected(native, passive, background)? == target.mode,
        "Refresh cannot change the selected mode."
    );
    let plan = plan_for(target.mode, passive, background);
    let grant = ownership::RegistrationGrant::capture(location)?
        .context("Original refresh requires a current Service Grant; revoked/blocked refresh is unsupported and never Rearms.")?;
    let original = instances::original(native, location, plan, &wanted(plan)?, until)?
        .context("The originally selected service is unavailable; no later runtime is adopted.")?;
    ensure!(
        current_target(location, plan, &original, &grant, &record)? == target,
        "Original refresh target changed."
    );
    let (source, identity) = instances::selected_image(Path::new(&plan.argv[0]))?;
    let mut fence = Fence {
        admission,
        location,
        background: true,
        grant: Some(grant),
        exclusion: None,
        until,
        source,
        identity,
        image: Path::new(&plan.argv[0]),
    };
    fence.check()?;
    marker(config, &plan.argv[0])?;
    fence.check()?;
    let boot = boot::current()?;
    let (pid, birth) = admission.caller.consumer();
    let scope = Scope {
        kind: journal::Kind::OriginalInstanceRefresh,
        mode: target.mode,
        journal: target.journal,
        owner_epoch: target.owner_epoch,
        selection: target.selection,
        grant_binding: fence
            .grant
            .as_ref()
            .expect("captured")
            .refresh_binding(location)?,
        original_generation: target.original_generation,
        original_image: target.original_image,
        replacement_image: target.replacement_image,
    };
    let reserved = record.reserve_refresh(
        request,
        original.observation(),
        scope.clone(),
        policy::mode::review::Consumer { pid, birth, boot },
    )?;
    let check = &mut || {
        fence.check()?;
        original.check()
    };
    write_anchor(config, location, plan, request, check)?;
    save(location, passive, &record, check)?;
    let mut session = OriginalSession {
        #[cfg(debug_assertions)]
        config,
        fence,
        native,
        plan,
        original,
        replacement: scope.replacement_image,
        ready: None,
    };
    journal::engine::run(&mut session, &mut record, reserved)?;
    let mut reply = view(location, &record, request);
    reply["read_only"] = false.into();
    reply["changed"] = true.into();
    reply["manager_changed"] = true.into();
    Ok(reply)
}

struct OriginalSession<'a> {
    #[cfg(debug_assertions)]
    config: &'a Config,
    fence: Fence<'a>,
    native: &'a Native,
    plan: &'a Plan,
    original: instances::Running,
    replacement: String,
    ready: Option<instances::Running>,
}
impl journal::engine::Session for OriginalSession<'_> {
    type Started = instances::Starting;
    fn phase(&mut self, phase: Budget) {
        self.fence.phase(phase);
    }
    fn check(&mut self) -> Result<()> {
        self.fence.check()?;
        self.original.image_files_current()?;
        instances::definition_current(
            self.native,
            self.plan,
            &wanted(self.plan)?,
            self.fence.until,
        )?;
        if let Some(ready) = &self.ready {
            ready.readiness(self.fence.location, self.plan, self.fence.until)?;
        }
        self.fence.check()
    }
    fn save(&mut self, record: &Record) -> Result<()> {
        let location = self.fence.location;
        let plan = self.plan;
        save(location, plan, record, &mut || self.check())
    }
    fn hold(&mut self) -> Result<()> {
        self.check()?;
        self.fence.exclusion = Some(Exclusion::Handoff(
            self.fence.location.try_handoff_lock()?.context(
                "Original resources have not drained; cancel only the same pre-manager intent.",
            )?,
        ));
        self.check()
    }
    fn drained(&mut self) -> Result<bool> {
        self.check()?;
        self.original
            .retirement_drained(self.fence.location, self.plan, self.fence.until)
    }
    fn pause(&mut self, request: &str) {
        #[cfg(debug_assertions)]
        held_pause::wait(self.config, request);
        #[cfg(not(debug_assertions))]
        let _ = request;
    }
    fn offer_stop(&mut self, record: &mut Record) -> Result<()> {
        let (native, location, plan, until) = (
            self.native,
            self.fence.location,
            self.plan,
            self.fence.until,
        );
        stop::offer(
            record,
            &self.original,
            &mut || self.fence.check(),
            |original, check| before_stop(native, location, plan, original, until, check),
            |record, check| save(location, plan, record, check),
        )
    }
    fn stop(&mut self) -> Result<()> {
        let (native, location, plan, until) = (
            self.native,
            self.fence.location,
            self.plan,
            self.fence.until,
        );
        stop::checked(
            &mut self.original,
            &mut || self.fence.check(),
            |original, check| before_stop(native, location, plan, original, until, check),
            |original, check| original.stop(check),
        )
    }
    fn stopped(&mut self) -> Result<bool> {
        self.check()?;
        instances::stopped(
            self.native,
            self.plan,
            &wanted(self.plan)?,
            &self.original,
            self.fence.until,
        )
    }
    fn start(&mut self) -> Result<Self::Started> {
        self.check()?;
        instances::start(
            self.native,
            self.fence.location,
            self.plan,
            &wanted(self.plan)?,
            self.fence.until,
            &mut || self.fence.check(),
        )
    }
    fn guid(started: &Self::Started) -> &str {
        started.id()
    }
    fn ready(&mut self, started: &mut Self::Started) -> Result<Option<Ready>> {
        self.check()?;
        let Some(ready) = started.poll(self.fence.location, self.plan, self.fence.until)? else {
            return Ok(None);
        };
        let (loaded, selected) = ready.refresh_images()?;
        ensure!(
            loaded == self.replacement && selected == loaded,
            "Original RunEx loaded another image."
        );
        let value = Ready {
            instance: ready.observation(),
            generation: ready.scope().1,
        };
        self.ready = Some(ready);
        self.check()?;
        Ok(Some(value))
    }
    fn release(&mut self) -> Result<()> {
        self.check()?;
        self.ready
            .as_ref()
            .context("Original readiness unavailable.")?
            .readiness(self.fence.location, self.plan, self.fence.until)?;
        self.fence.check()?;
        drop(self.fence.exclusion.take());
        self.check()
    }
}
