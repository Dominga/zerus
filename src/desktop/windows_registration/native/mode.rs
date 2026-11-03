//! Explicit owned-task mode changes. No generic registration request enters here.
use super::*;
use crate::{
    config::Config,
    desktop::{
        ownership, registration, revoke,
        store::{HandoffLock, Location},
        ui_authority,
    },
    platform,
};
use policy::mode::{Mode, Phase, Record, RECORD_BYTES};
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

pub(in crate::desktop::windows_registration) const ENTRY: &str = "__desktop-registration-mode";
const MODE_ABI: &str = "desktop-registration-mode-v1";
const RECORD: &str = "windows-registration-mode.json";

struct Admission {
    scope: ui_authority::Scope,
    caller: ui_authority::registration::Caller,
}
impl Admission {
    fn check(&self) -> Result<()> {
        self.caller.check(&self.scope)
    }
}

fn environment(config: &Config, binary: &str) -> Result<Vec<(String, String)>> {
    let mut values = super::super::helper_environment("")?;
    for (key, value) in [
        ("HOME", config.home.clone()),
        ("USERPROFILE", config.home.clone()),
        (
            "HGS_CONFIG_DIR",
            config
                .dir
                .to_str()
                .context("Config path is not Unicode.")?
                .into(),
        ),
        (
            "HGS_STATE_DIR",
            config
                .state
                .to_str()
                .context("State path is not Unicode.")?
                .into(),
        ),
        ("HGS_EXECUTABLE", binary.into()),
    ] {
        values.push((key.into(), value));
    }
    if let Ok(path) = std::env::var("LOCALAPPDATA") {
        values.push(("LOCALAPPDATA".into(), path));
    }
    Ok(values)
}

pub(in crate::desktop::windows_registration) fn command(
    config: &Config,
    location: &Location,
    request: Value,
) -> Result<Value> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    let scope = ui_authority::Scope::new(config, location);
    // Capture the real caller BEFORE the helper environment is sanitized.
    let caller = ui_authority::registration::Caller::capture(&scope)?;
    let attached = caller.source() != "installed_frontend";
    let admission = Arc::new(Admission { scope, caller });
    admission.check()?;
    let plan = registration::windows_plan(config, location)?;
    let binary = plan["argv"][0]
        .as_str()
        .context("Selected desktop CLI unavailable.")?;
    let env = environment(config, binary)?;
    let frame = serde_json::to_vec(&request)?;
    ensure!(
        !frame.is_empty() && frame.len() <= CAP,
        "Mode request exceeds its bound."
    );
    let invocation = zerus_client_core::actions::Invocation {
        args: vec![ENTRY.into(), frame.len().to_string()],
        stdin: Some(frame),
        timeout: Duration::from_secs(15),
        request_id: None,
    };
    let proof = admission.clone();
    let child_env = env.clone();
    let references: Vec<_> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let output = zerus_client_core::run::run_owned(
        &std::env::current_exe()?, &invocation, &references, &AtomicBool::new(false),
        move |command| {
            proof.check().map_err(std::io::Error::other)?;
            command.env_clear().envs(child_env).creation_flags(if attached { 0 } else { CREATE_NO_WINDOW }).spawn()
        }, admission,
    ).map_err(|error| anyhow::anyhow!("Desktop mode result is unavailable ({error:?}); inspect status and retain the original pending UUID."))?;
    ensure!(output.len() <= CAP, "Mode response exceeds its bound.");
    let mut reply: Value = serde_json::from_slice(&output)?;
    ensure!(reply["abi"] == MODE_ABI, "Mode response ABI differs.");
    reply["operation"] = request["operation"].clone();
    reply["requested_mode"] = request["mode"].clone();
    if reply["error"].is_string() {
        reply["configuration"] = location.configuration.clone().into();
        reply["task_name"] = plan["task_name"].clone();
        reply["ok"] = false.into();
        let record = pending_status(location, &plan)?;
        reply["pending_transition"] = record["pending_transition"].clone();
        let pending = reply["pending_transition"].is_object();
        reply["changed"] = if pending { Value::Null } else { false.into() };
        reply["manager_changed"] = reply["changed"].clone();
        reply["outcome"] = if pending { "unknown" } else { "refused" }.into();
        return Ok(reply);
    }
    ensure!(
        reply["configuration"] == location.configuration,
        "Mode response differs from the selected registration."
    );
    let pending = reply
        .as_object_mut()
        .expect("object")
        .remove("pending")
        .unwrap_or(Value::Null);
    reply["ok"] = pending.is_null().into();
    reply["outcome"] = if pending.is_null() {
        "settled"
    } else {
        "pending"
    }
    .into();
    reply["pending_transition"] = pending;
    Ok(reply)
}

fn plans(config: &Config, location: &Location, sid: &str) -> Result<(Plan, Plan)> {
    let passive = registration::windows_plan(config, location)?;
    let base = Plan::parse(&json!({"schema":1,"operation":"check","plan":passive}), sid)?;
    let background = registration::windows_plan_mode(config, location, true)?;
    let mut other = base.clone();
    other.argv.push("--background".into());
    ensure!(
        background["argv"] == serde_json::to_value(&other.argv)?
            && background["configuration"] == base.configuration,
        "Maintained mode definitions differ."
    );
    other.definition = background["definition"]
        .as_str()
        .context("Background definition unavailable.")?
        .into();
    Ok((base, other))
}
fn wanted(plan: &Plan) -> Result<Shape> {
    Ok(plan.shape(crate::host::spawn::command_line(&plan.argv[1..])?))
}
fn load(location: &Location, plan: &Plan) -> Result<Record> {
    match crate::desktop::handoff::read_private(&location.root.join(RECORD), RECORD_BYTES) {
        Ok(bytes) => Record::decode(&bytes, plan),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Record::empty(plan)
        }
        Err(error) => Err(error),
    }
}
pub(in crate::desktop::windows_registration) fn pending_status(
    location: &Location,
    preview: &Value,
) -> Result<Value> {
    let sid = crate::host::token::user_sid()?;
    let plan = Plan::parse(
        &json!({"schema":1,"operation":"status","plan":preview}),
        &sid,
    )?;
    let record = load(location, &plan)?;
    Ok(
        json!({"pending_transition":record.pending,"cancelled_request_id":record.cancelled,"completed_request_id":record.completed}),
    )
}
fn save(
    location: &Location,
    plan: &Plan,
    record: &Record,
    fence: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    fence()?;
    location.prepare()?;
    let bytes = record.encode(plan)?;
    let mut file = tempfile::NamedTempFile::new_in(&location.root)?;
    platform::make_private(file.as_file())?;
    ensure!(
        platform::verify_private_file(file.as_file())?,
        "Mode record is not private."
    );
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    fence()?;
    platform::persist(file, location.root.join(RECORD)).map_err(|error| error.error)?;
    platform::sync_dir(&location.root)?;
    fence()
}
fn selected(native: &mut Native, passive: &Plan, background: &Plan) -> Result<Mode> {
    let current = native
        .read(&passive.name)?
        .context("Install the maintained passive task before changing mode.")?;
    if current.validate(&wanted(passive)?, &passive.sid).is_ok() {
        return Ok(Mode::Passive);
    }
    current.validate(&wanted(background)?, &background.sid)?;
    Ok(Mode::Background)
}

enum Exclusion {
    Rollback(Box<hold_child::Hold>),
    Handoff(HandoffLock),
}
struct Fence<'a> {
    admission: &'a Admission,
    location: &'a Location,
    background: bool,
    exclusion: Option<Exclusion>,
    until: Instant,
    source: std::fs::File,
    identity: platform::FileIdentity,
    image: &'a Path,
}
impl Fence<'_> {
    fn check(&mut self) -> Result<()> {
        ensure!(
            Instant::now() < self.until,
            "Mode transition deadline reached; retain its original UUID."
        );
        self.admission.check()?;
        let held = platform::FileIdentity::of(&self.source)?;
        let (_, current) = instances::selected_image(self.image)?;
        ensure!(
            held.device == self.identity.device
                && held.inode == self.identity.inode
                && current.device == self.identity.device
                && current.inode == self.identity.inode,
            "Selected installed CLI changed during the transition."
        );
        if let Some(exclusion) = &mut self.exclusion {
            match exclusion {
                Exclusion::Rollback(hold) => hold.check()?,
                Exclusion::Handoff(lock) => {
                    lock.verify(self.location)?;
                    if self.background {
                        let (db, _verification) = revoke::hold::database(self.location)?;
                        ensure!(
                            zerus_desktop_ledger::operations::totals(&db)?.undrained == 0,
                            "Desktop operation resources have not drained."
                        );
                    }
                }
            }
        }
        if self.background {
            ensure!(
                ownership::registration_granted(self.location)?,
                "Grant or Rearm from Zerus Settings is required before background mode."
            );
        }
        self.admission.check()
    }
}

fn marker(config: &Config, binary: &str) -> Result<()> {
    let invocation = zerus_client_core::actions::Invocation {
        args: vec!["--help".into()],
        stdin: None,
        timeout: Duration::from_secs(2),
        request_id: None,
    };
    let env = environment(config, binary)?;
    let refs: Vec<_> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let result =
        zerus_client_core::run::run_hidden_limited(Path::new(binary), &invocation, &refs, CAP)
            .map_err(|error| {
                anyhow::anyhow!("Selected helper ABI probe is unavailable: {error:?}")
            })?;
    let zerus_client_core::actions::Exit::Code {
        code: 0, stdout, ..
    } = result
    else {
        bail!("Selected helper ABI probe failed.");
    };
    ensure!(
        String::from_utf8(stdout)?
            .lines()
            .any(|line| line.trim() == "Desktop revoke hold ABI: desktop-revoke-hold-v1"),
        "Update the selected CLI before changing desktop mode."
    );
    Ok(())
}

fn revoke_original(
    location: &Location,
    request_id: &str,
    original: Option<&instances::Running>,
) -> Result<Value> {
    let command = revoke::cli(&["--json", "--request-id", request_id, "--reason", "rollback"])?;
    match original {
        Some(original) => original.revoke(location, revoke::payload(&command)?),
        None => revoke::offline(location, &command),
    }
}

fn rollback_result(
    location: &Location,
    id: &str,
    original: Option<&instances::Running>,
) -> Result<Value> {
    if let Some(original) = original {
        original.check()?;
    }
    let value = revoke::rollback_receipt(location, id)?;
    if let Some(original) = original {
        original.check()?;
    }
    Ok(value)
}

struct Runtime<'a> {
    config: &'a Config,
    location: &'a Location,
    native: &'a mut Native,
    scheduler: &'a ITaskService,
    admission: &'a Admission,
    passive: &'a Plan,
    background: &'a Plan,
}
fn effect(runtime: Runtime<'_>, desired: Mode, until: Instant) -> Result<Value> {
    let Runtime {
        config,
        location,
        native,
        scheduler,
        admission,
        passive,
        background,
    } = runtime;
    let from = selected(native, passive, background)?;
    let old = if from == Mode::Passive {
        passive
    } else {
        background
    };
    let new = if desired == Mode::Passive {
        passive
    } else {
        background
    };
    let mut record = load(location, passive)?;
    let resuming = record.pending.clone();
    if resuming.as_ref().is_some_and(|pending| {
        pending.desired != desired
            || !matches!(
                pending.phase,
                Phase::Prepared | Phase::RevokeOffered | Phase::Held
            )
    }) {
        return Ok(
            json!({"abi":MODE_ABI,"schema":1,"configuration":location.configuration,"task_name":passive.name,"selected_mode":from.name(),"pending":record.pending,"changed":false,"code":"original_transition_pending"}),
        );
    }
    let (source, identity) = instances::selected_image(Path::new(&new.argv[0]))?;
    let mut fence = Fence {
        admission,
        location,
        background: desired == Mode::Background,
        exclusion: None,
        until,
        source,
        identity,
        image: Path::new(&new.argv[0]),
    };
    fence.check()?;
    let mut original = instances::original(native, location, old, &wanted(old)?, until)?;
    let observation = original.as_ref().map(instances::Running::observation);
    if let Some(pending) = &resuming {
        ensure!(
            pending.original == observation,
            "Original task or service changed; retain the pending UUID and inspect it."
        );
    }
    if resuming.is_none() && desired == Mode::Background && from == desired && original.is_some() {
        return Ok(
            json!({"abi":MODE_ABI,"schema":1,"configuration":location.configuration,"task_name":passive.name,"selected_mode":desired.name(),"running_mode":desired.name(),"pending":null,"changed":false}),
        );
    }
    marker(config, &new.argv[0])?;
    let id = if let Some(pending) = &resuming {
        pending.request_id.clone()
    } else {
        let id = record.begin(desired, observation)?.request_id.clone();
        save(location, passive, &record, &mut || fence.check())?;
        id
    };
    let database = location.root.join("state.sqlite3").try_exists()?;
    if desired == Mode::Passive && database {
        let mut receipt = if record.pending.as_ref().expect("pending").phase == Phase::Prepared {
            record.advance(Phase::Prepared, Phase::RevokeOffered)?;
            save(location, passive, &record, &mut || fence.check())?;
            let value = revoke_original(location, &id, original.as_ref())?;
            if value["checkpointed"] == true {
                value
            } else {
                rollback_result(location, &id, original.as_ref())?
            }
        } else {
            rollback_result(location, &id, original.as_ref())?
        };
        loop {
            fence.check()?;
            if receipt["accepted"] == true
                && receipt["durable"] == true
                && receipt["revoked"] == true
                && receipt["checkpointed"] == true
            {
                break;
            }
            ensure!(receipt["durable"]==true,"Original rollback was not applied; inspect its result or cancel the exact saved UUID after a current Service grant is verified.");
            std::thread::sleep(Duration::from_millis(50));
            // This is a KNOWN committed, prepared authority reduction, with
            // the same UUID and original process. The maintained revoke API
            // reconciles that original drain; an absent/unknown reservation is
            // never resent and a new writer still fails its native epoch check.
            receipt = revoke_original(location, &id, original.as_ref())?;
            if receipt["checkpointed"] != true {
                receipt = rollback_result(location, &id, original.as_ref())?;
            }
        }
        fence.exclusion = Some(Exclusion::Rollback(Box::new(hold_child::Hold::start(
            Path::new(&new.argv[0]),
            &environment(config, &new.argv[0])?,
            &receipt,
            &fence.identity,
            admission.caller.source() != "installed_frontend",
            until,
        )?)));
        if record.pending.as_ref().expect("pending").phase == Phase::RevokeOffered {
            record.advance(Phase::RevokeOffered, Phase::Held)?;
        }
    } else {
        // Passive bootstrap has no old database/receipt. Background enablement
        // already requires an actual stored Service grant. Both exclude new
        // operation/native owners and Rearm while replacing the passive actor.
        location.prepare()?;
        let lock = location
            .try_handoff_lock()?
            .context("Desktop native or operation resources have not drained.")?;
        if desired == Mode::Passive {
            ensure!(
                !location.root.join("state.sqlite3").try_exists()?
                    && !location
                        .root
                        .join(zerus_desktop_migration::HANDOFF_FILE)
                        .try_exists()?,
                "Passive bootstrap state changed."
            );
        }
        fence.exclusion = Some(Exclusion::Handoff(lock));
        fence.check()?;
        if record.pending.as_ref().expect("pending").phase == Phase::Prepared {
            record.advance(Phase::Prepared, Phase::Held)?;
        }
    }
    save(location, passive, &record, &mut || fence.check())?;
    if let Some(original) = &mut original {
        record.advance(Phase::Held, Phase::StopOffered)?;
        save(location, passive, &record, &mut || fence.check())?;
        instances::stop_original(native, old, &wanted(old)?, original, &mut || fence.check())?;
        while !instances::stopped(native, old, &wanted(old)?, original)? {
            fence.check()?;
            std::thread::sleep(Duration::from_millis(50));
        }
        record.advance(Phase::StopOffered, Phase::Stopped)?;
    } else {
        record.advance(Phase::Held, Phase::Stopped)?;
    }
    save(location, passive, &record, &mut || fence.check())?;
    record.advance(Phase::Stopped, Phase::UpdateOffered)?;
    save(location, passive, &record, &mut || fence.check())?;
    instances::replace(
        native,
        scheduler,
        location,
        (old, &wanted(old)?),
        (new, &wanted(new)?),
        &mut || fence.check(),
    )?;
    record.advance(Phase::UpdateOffered, Phase::Updated)?;
    save(location, passive, &record, &mut || fence.check())?;
    record.advance(Phase::Updated, Phase::StartOffered)?;
    save(location, passive, &record, &mut || fence.check())?;
    let mut started =
        instances::start(native, location, new, &wanted(new)?, &mut || fence.check())?;
    record.pending.as_mut().expect("pending").started = Some(started.id().into());
    save(location, passive, &record, &mut || fence.check())?;
    let ready = loop {
        fence.check()?;
        if let Some(ready) = started.poll(location, new, until)? {
            break ready;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    ready.check()?;
    fence.check()?;
    record.advance(Phase::StartOffered, Phase::Ready)?;
    save(location, passive, &record, &mut || fence.check())?;
    // Keep the ORIGINAL hold through actual passive SDK/process/IPC readiness.
    if let Some(Exclusion::Rollback(hold)) = &mut fence.exclusion {
        hold.finish(until)?;
    } else {
        drop(fence.exclusion.take());
    }
    admission.check()?;
    ready.check()?;
    record.completed = Some(id.clone());
    record.pending = None;
    save(location, passive, &record, &mut || admission.check())?;
    Ok(
        json!({"abi":MODE_ABI,"schema":1,"configuration":location.configuration,"task_name":passive.name,"selected_mode":desired.name(),"running_mode":desired.name(),"pending":null,"request_id":id,"changed":true,"instance":ready.observation()}),
    )
}

fn operation(args: &[String]) -> Result<Value> {
    ensure!(args.len() == 1, "Invalid mode helper arguments.");
    let length: usize = args[0].parse()?;
    ensure!(length.to_string() == args[0], "Invalid mode helper length.");
    let request = policy::read_frame(&mut std::io::stdin().lock(), length)?;
    let op = request["operation"].as_str().unwrap_or("");
    ensure!(
        request["schema"] == 1 && matches!(op, "switch" | "cancel_rollback"),
        "Unsupported mode request."
    );
    let keys = request.as_object().context("Invalid mode request.")?;
    ensure!(
        keys.len() == 3
            && if op == "switch" {
                keys.contains_key("mode")
            } else {
                keys.contains_key("request_id")
            },
        "Unexpected mode request fields."
    );
    let config = Config::load().map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let location = Location::new(&config)?;
    let scope = ui_authority::Scope::new(&config, &location);
    let caller = ui_authority::registration::Caller::capture(&scope)?;
    let admission = Admission { scope, caller };
    admission.check()?;
    let sid = crate::host::token::user_sid()?;
    let (passive, background) = plans(&config, &location, &sid)?;
    let _lock = Lock::acquire(&sid)?;
    let until = Instant::now() + Duration::from_secs(12);
    let _apartment = Apartment::new()?;
    let scheduler: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }?;
    unsafe {
        scheduler.Connect(
            &VARIANT::default(),
            &VARIANT::default(),
            &VARIANT::default(),
            &VARIANT::default(),
        )
    }?;
    let mut native = Native {
        folder: unsafe { scheduler.GetFolder(&BSTR::from("\\")) }?,
    };
    if op == "cancel_rollback" {
        admission.check()?;
        ensure!(
            ownership::registration_granted(&location)?,
            "Cancellation requires a fresh Service Grant/Rearm; it changes metadata only."
        );
        let mode = selected(&mut native, &passive, &background)?;
        let mut record = load(&location, &passive)?;
        let id = request["request_id"]
            .as_str()
            .context("Original rollback UUID required.")?;
        record.cancel(id)?;
        save(&location, &passive, &record, &mut || {
            admission.check()?;
            ensure!(
                ownership::registration_granted(&location)?,
                "Service grant changed."
            );
            Ok(())
        })?;
        return Ok(
            json!({"abi":MODE_ABI,"schema":1,"configuration":location.configuration,"task_name":passive.name,"selected_mode":mode.name(),"pending":null,"cancelled_request_id":id,"changed":true,"manager_changed":false}),
        );
    }
    let desired = Mode::parse(request["mode"].as_str().unwrap_or(""))?;
    effect(
        Runtime {
            config: &config,
            location: &location,
            native: &mut native,
            scheduler: &scheduler,
            admission: &admission,
            passive: &passive,
            background: &background,
        },
        desired,
        until,
    )
}

pub(in crate::desktop::windows_registration) fn serve(args: &[String]) -> i32 {
    let value = match operation(args) {
        Ok(value) => value,
        Err(error) => {
            json!({"abi":MODE_ABI,"schema":1,"code":"mode_transition_unconfirmed","error":format!("{error:#}")})
        }
    };
    // An error receipt is data; callers inspect the retained transition. A
    // successful transport never turns an unknown manager result into success.
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let bytes=match serde_json::to_vec(&value) {
        Ok(bytes) if bytes.len()<=CAP=>bytes,
        _=>serde_json::to_vec(&json!({"abi":MODE_ABI,"schema":1,"code":"mode_response_unavailable","error":"Inspect the retained original transition."})).expect("fixed response"),
    };
    if out.write_all(&bytes).is_err() || out.write_all(b"\n").is_err() || out.flush().is_err() {
        1
    } else {
        0
    }
}
