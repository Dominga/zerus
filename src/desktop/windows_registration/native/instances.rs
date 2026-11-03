//! Original Task Scheduler instances and their independently verified service.
//! These objects are private native custody, never decoded request authority.
use super::*;
use crate::desktop::{protocol, store::Location, transport};
use std::{
    fs::File,
    io::{BufReader, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use zerus_client_core::desktop::windows::process::Original;

fn task(folder: &ITaskFolder, plan: &Plan, wanted: &Shape) -> Result<IRegisteredTask> {
    let task = unsafe { folder.GetTask(&BSTR::from(&plan.name)) }?;
    let (owner, acl) = security(&task)?;
    Task {
        shape: shape(&unsafe { task.Definition() }?)?,
        owner,
        acl,
        enabled: unsafe { task.Enabled() }?.as_bool(),
        state: unsafe { task.State() }?.0,
    }
    .validate(wanted, &plan.sid)?;
    Ok(task)
}

fn guid(instance: &IRunningTask) -> Result<String> {
    let value = text(&unsafe { instance.InstanceGuid() }?)?;
    let id = uuid::Uuid::parse_str(&value).context("Task instance identity unavailable.")?;
    ensure!(!id.is_nil(), "Task instance identity unavailable.");
    Ok(value)
}

fn instances(task: &IRegisteredTask) -> Result<Vec<IRunningTask>> {
    let list = unsafe { task.GetInstances(0) }?;
    let count = unsafe { list.Count() }?;
    // The maintained task's IgnoreNew policy admits only one instance. Extra
    // instances are an uncertain topology, never permission to stop them all.
    ensure!(
        (0..=1).contains(&count),
        "Multiple task instances require inspection."
    );
    (1..=count)
        .map(|index| Ok(unsafe { list.get_Item(&VARIANT::from(index)) }?))
        .collect()
}

fn session(pid: u32) -> Result<u32> {
    let mut session = 0;
    ensure!(
        unsafe {
            windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId(pid, &mut session)
        } != 0
            && session > 0,
        "Task process session unavailable."
    );
    Ok(session)
}

pub(super) fn selected_image(path: &Path) -> Result<(File, crate::platform::FileIdentity)> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    };
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let (id, metadata) = crate::platform::FileIdentity::with_metadata(&file)?;
    ensure!(
        metadata.is_file()
            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
            && id.inode != 0,
        "Selected desktop image unavailable."
    );
    Ok((file, id))
}

struct Service {
    process: Original,
    _image: File,
    image: crate::platform::FileIdentity,
    selected: PathBuf,
    epoch: String,
}
impl Service {
    fn observe(
        location: &Location,
        plan: &Plan,
        engine: u32,
        until: Instant,
    ) -> Result<Option<Self>> {
        let Some(stream) = crate::desktop::connection(location, "status", None)? else {
            return Ok(None);
        };
        let pid = transport::peer_pid(&stream)?;
        let process = Original::open(pid)?;
        ensure!(
            process.own && process.current(),
            "Desktop process identity unavailable."
        );
        let current_session = crate::host::token::windows_session()?;
        ensure!(
            session(pid)? == current_session,
            "Desktop task belongs to another session."
        );
        let bytes = protocol::line(&mut BufReader::new(stream), protocol::FRAME_BYTES)?
            .context("Desktop readiness disconnected.")?;
        let reply: Value = serde_json::from_slice(&bytes)?;
        let value = &reply["value"];
        let lifecycle = &value["lifecycle"];
        ensure!(
            reply["abi"] == crate::desktop::ABI
                && lifecycle["abi"] == "desktop-lifecycle-v1"
                && lifecycle["configuration"] == location.configuration
                && lifecycle["ipc_ready"] == true
                && lifecycle["pid"].as_u64() == Some(pid as u64),
            "Desktop readiness does not match the original process."
        );
        let epoch = lifecycle["epoch"]
            .as_str()
            .context("Desktop epoch unavailable.")?;
        ensure!(
            uuid::Uuid::parse_str(epoch).is_ok_and(|id| id.to_string() == epoch),
            "Desktop epoch unavailable."
        );
        let wanted_mode = if plan.argv.last().is_some_and(|v| v == "--background") {
            "background"
        } else {
            "passive"
        };
        ensure!(
            lifecycle["mode"] == wanted_mode,
            "Desktop running mode differs from the selected task."
        );
        let arguments = process.arguments()?;
        ensure!(
            arguments == plan.argv,
            "Desktop process command differs from the owned task."
        );
        // EnginePID alone never supplies a service PID. Require the SAME
        // process independently verified through the original desktop pipe.
        // A common scheduler/ancestor engine cannot bind a specific instance.
        ensure!(Instant::now() < until && engine == process.pid,
            "Task engine is not the verified desktop process; this instance topology cannot be stopped safely.");
        // The original instance/pipe/kernel binding above selects this exact
        // runtime. An installer may have moved its image aside during upgrade;
        // retain that image independently of the replacement's selected file.
        let (image, identity) = selected_image(Path::new(&process.image))?;
        let selected = PathBuf::from(&process.image);
        let value = Self {
            process,
            _image: image,
            image: identity,
            selected,
            epoch: epoch.into(),
        };
        value.check()?;
        Ok(Some(value))
    }
    fn check(&self) -> Result<()> {
        ensure!(self.process.current(), "Original desktop process changed.");
        let actual = crate::platform::FileIdentity::of(&self._image)?;
        let (_, selected) = selected_image(&self.selected)?;
        ensure!(
            actual.device == self.image.device
                && actual.inode == self.image.inode
                && selected.device == self.image.device
                && selected.inode == self.image.inode,
            "Original desktop image changed."
        );
        ensure!(self.process.current(), "Original desktop process changed.");
        Ok(())
    }
}

/// The original interface, immutable instance GUID and actual process handles.
/// No Clone/serde or lookup-by-saved-PID constructor is provided.
pub(super) struct Running {
    instance: IRunningTask,
    guid: String,
    path: String,
    engine: u32,
    service: Service,
    stopped: bool,
}
impl Running {
    pub(super) fn revoke(&self, location: &Location, request: Value) -> Result<Value> {
        ensure!(
            matches!(request["op"].as_str(), Some("revoke" | "revoke_status")),
            "Only the original rollback or its read-only result may use this channel."
        );
        let id = request["request_id"]
            .as_str()
            .context("Original rollback UUID unavailable.")?
            .to_owned();
        self.check()?;
        let mut stream =
            transport::connect(location)?.context("Original desktop service disconnected.")?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(crate::desktop::TIMEOUT))?;
        ensure!(
            transport::peer_pid(&stream)? == self.service.process.pid,
            "Desktop service changed before rollback; no request was offered."
        );
        self.check()?;
        let hello = json!({"abi":crate::desktop::ABI,"configuration":location.configuration,"op":"request","request":request});
        stream.write_all(&protocol::encode(&hello)?)?;
        let bytes = protocol::line(&mut BufReader::new(stream), protocol::FRAME_BYTES)?
            .context("Original rollback result is unknown.")?;
        let reply: Value = serde_json::from_slice(&bytes)?;
        ensure!(
            reply["abi"] == crate::desktop::ABI && reply["t"] != "error",
            "Original rollback result is unavailable."
        );
        let receipt = reply["value"].clone();
        ensure!(
            receipt["request_id"] == id,
            "Original rollback receipt identity differs."
        );
        if receipt["checkpointed"] == true {
            ensure!(
                receipt["configuration"] == location.configuration
                    && receipt
                        .get("writer_epoch")
                        .is_some_and(|epoch| epoch.is_null()
                            || epoch
                                .as_str()
                                .is_some_and(|epoch| uuid::Uuid::parse_str(epoch)
                                    .is_ok_and(|id| id.to_string() == epoch))),
                "Original rollback configuration or writer epoch is unavailable."
            );
        }
        ensure!(
            !matches!(
                receipt["code"].as_str(),
                Some("writer_epoch_changed" | "writer_authority_changed" | "request_conflict")
            ),
            "Original rollback authority changed; retain its UUID for explicit cancellation."
        );
        self.check()?;
        Ok(receipt)
    }
    pub(super) fn observation(
        &self,
    ) -> crate::desktop::windows_registration_policy::mode::ObservedInstance {
        crate::desktop::windows_registration_policy::mode::ObservedInstance {
            guid: self.guid.clone(),
            pid: self.service.process.pid,
            birth: self.service.process.birth,
            epoch: self.service.epoch.clone(),
        }
    }
    fn bind(
        instance: IRunningTask,
        location: &Location,
        plan: &Plan,
        until: Instant,
    ) -> Result<Option<Self>> {
        let id = guid(&instance)?;
        let path = text(&unsafe { instance.Path() }?)?;
        ensure!(
            path == format!(r"\{}", plan.name),
            "Task instance path differs."
        );
        unsafe { instance.Refresh() }?;
        let engine = unsafe { instance.EnginePID() }?;
        if engine == 0 {
            return Ok(None);
        }
        let Some(service) = Service::observe(location, plan, engine, until)? else {
            return Ok(None);
        };
        let value = Self {
            instance,
            guid: id,
            path,
            engine,
            service,
            stopped: false,
        };
        value.check()?;
        Ok(Some(value))
    }
    pub(super) fn check(&self) -> Result<()> {
        ensure!(!self.stopped, "Original task stop was already offered.");
        self.service.check()?;
        unsafe { self.instance.Refresh() }?;
        ensure!(
            guid(&self.instance)? == self.guid
                && unsafe { self.instance.State() }? == TASK_STATE_RUNNING
                && text(&unsafe { self.instance.Path() }?)? == self.path
                && unsafe { self.instance.EnginePID() }? == self.engine,
            "Original task instance changed."
        );
        self.service.check()
    }
    pub(super) fn stop(&mut self, fence: &mut impl FnMut() -> Result<()>) -> Result<()> {
        fence()?;
        self.check()?;
        fence()?;
        // Mark before calling COM: a failed/unknown result is never replayed.
        self.stopped = true;
        unsafe { self.instance.Stop() }
            .context("Original task stop result is unknown; inspect the pending transition.")?;
        fence()
    }
    pub(super) fn exited(&self, selected: &IRegisteredTask) -> Result<bool> {
        ensure!(self.stopped, "Original task was not stopped.");
        if !self.service.process.exited() {
            return Ok(false);
        }
        Ok(instances(selected)?
            .iter()
            .all(|instance| guid(instance).is_ok_and(|id| id != self.guid)))
    }
}

pub(super) fn original(
    native: &Native,
    location: &Location,
    plan: &Plan,
    wanted: &Shape,
    until: Instant,
) -> Result<Option<Running>> {
    let selected = task(&native.folder, plan, wanted)?;
    let mut values = instances(&selected)?;
    match values.pop() {
        Some(instance) => Running::bind(instance, location, plan, until)?
            .map(Some)
            .context("Task is running but its original desktop process cannot be verified."),
        None => {
            ensure!(
                crate::desktop::connection(location, "status", None)?.is_none(),
                "A desktop process exists outside this task instance."
            );
            Ok(None)
        }
    }
}

pub(super) fn stopped(
    native: &Native,
    plan: &Plan,
    wanted: &Shape,
    original: &Running,
) -> Result<bool> {
    original.exited(&task(&native.folder, plan, wanted)?)
}

pub(super) fn stop_original(
    native: &Native,
    plan: &Plan,
    wanted: &Shape,
    original: &mut Running,
    fence: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    fence()?;
    let selected = task(&native.folder, plan, wanted)?;
    let values = instances(&selected)?;
    ensure!(
        values.len() == 1 && guid(&values[0])? == original.guid,
        "Original task selection changed before stop."
    );
    original.stop(fence)
}

pub(super) fn replace(
    native: &Native,
    service: &ITaskService,
    location: &Location,
    old: (&Plan, &Shape),
    new: (&Plan, &Shape),
    fence: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    let (old_plan, old_shape) = old;
    let (new_plan, new_shape) = new;
    ensure!(
        old_plan.name == new_plan.name
            && old_plan.sid == new_plan.sid
            && old_plan.configuration == new_plan.configuration
            && old_plan.argv[..7] == new_plan.argv[..7],
        "A mode transition cannot change the installed task identity or paths."
    );
    let definition = unsafe { service.NewTask(0) }?;
    unsafe { definition.SetXmlText(&BSTR::from(&new_plan.definition)) }?;
    ensure!(
        shape(&definition)? == *new_shape,
        "Replacement task definition differs."
    );
    fence()?;
    let selected = task(&native.folder, old_plan, old_shape)?;
    ensure!(
        instances(&selected)?.is_empty(),
        "Original task instances have not drained."
    );
    ensure!(
        crate::desktop::connection(location, "status", None)?.is_none(),
        "Desktop endpoint still has an owner."
    );
    let sddl = unsafe { selected.GetSecurityDescriptor(5) }?;
    fence()?;
    // Preserve the verified owner/DACL and do not execute registration triggers.
    // This is UPDATE only: a disappearing task must not become an implicit install.
    unsafe {
        native.folder.RegisterTask(
            &BSTR::from(&new_plan.name),
            &BSTR::from(&new_plan.definition),
            TASK_UPDATE.0 | TASK_IGNORE_REGISTRATION_TRIGGERS.0 | TASK_DONT_ADD_PRINCIPAL_ACE.0,
            &VARIANT::from(new_plan.sid.as_str()),
            &VARIANT::default(),
            TASK_LOGON_INTERACTIVE_TOKEN,
            &VARIANT::from(sddl.to_string().as_str()),
        )
    }
    .context("Task update result is unknown; retain the pending transition.")?;
    fence()?;
    task(&native.folder, new_plan, new_shape)?;
    Ok(())
}

pub(super) struct Starting {
    instance: IRunningTask,
    guid: String,
    path: String,
    engine: Option<u32>,
}
impl Starting {
    pub(super) fn id(&self) -> &str {
        &self.guid
    }
    pub(super) fn poll(
        &mut self,
        location: &Location,
        plan: &Plan,
        until: Instant,
    ) -> Result<Option<Running>> {
        unsafe { self.instance.Refresh() }?;
        ensure!(
            guid(&self.instance)? == self.guid
                && text(&unsafe { self.instance.Path() }?)? == self.path,
            "Original started task instance changed."
        );
        let engine = unsafe { self.instance.EnginePID() }?;
        if engine == 0 {
            return Ok(None);
        }
        if let Some(original) = self.engine {
            ensure!(engine == original, "Original started task engine changed.");
        } else {
            self.engine = Some(engine);
        }
        // Clone retains this same returned COM interface; it never reselects
        // an instance by task name, saved GUID or process ID.
        Running::bind(self.instance.clone(), location, plan, until)
    }
}

pub(super) fn start(
    native: &Native,
    location: &Location,
    plan: &Plan,
    wanted: &Shape,
    fence: &mut impl FnMut() -> Result<()>,
) -> Result<Starting> {
    let session = crate::host::token::windows_session()?;
    ensure!(
        session > 0 && session <= i32::MAX as u32,
        "Interactive task session unavailable."
    );
    fence()?;
    let selected = task(&native.folder, plan, wanted)?;
    ensure!(
        instances(&selected)?.is_empty(),
        "A task instance already exists; no second start is allowed."
    );
    ensure!(
        crate::desktop::connection(location, "status", None)?.is_none(),
        "An existing desktop endpoint requires inspection."
    );
    ensure!(
        crate::host::token::windows_session()? == session,
        "Interactive task session changed."
    );
    fence()?;
    let instance = unsafe {
        selected.RunEx(
            &VARIANT::default(),
            TASK_RUN_USE_SESSION_ID.0,
            session as i32,
            &BSTR::from(plan.sid.as_str()),
        )
    }
    .context("Task start result is unknown; do not repeat this start.")?;
    let id = guid(&instance)?;
    let path = text(&unsafe { instance.Path() }?)?;
    ensure!(
        path == format!(r"\{}", plan.name),
        "Started task path differs."
    );
    fence()?;
    Ok(Starting {
        instance,
        guid: id,
        path,
        engine: None,
    })
}
