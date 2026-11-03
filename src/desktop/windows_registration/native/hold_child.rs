//! The original revoke-hold child, retained until EOF release and physical join.
use super::*;
use crate::platform;
use std::{
    io::Read,
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::Path,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{SetHandleInformation, ERROR_BROKEN_PIPE, FILETIME, HANDLE_FLAG_INHERIT},
    System::{
        Pipes::PeekNamedPipe,
        Threading::{GetProcessTimes, CREATE_NO_WINDOW},
    },
};
use zerus_client_core::desktop::windows::process::Original;

const HOLD_ABI: &str = "desktop-revoke-hold-v1";

fn read_available(
    pipe: &mut (impl AsRawHandle + Read),
    bytes: &mut Vec<u8>,
    cap: usize,
) -> Result<bool> {
    let mut available = 0;
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut available,
            ptr::null_mut(),
        )
    } == 0
    {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
            return Ok(true);
        }
        return Err(error.into());
    }
    ensure!(
        bytes.len() + available as usize <= cap,
        "Original hold output exceeds its bound."
    );
    let mut buffer = [0; 4096];
    while available > 0 {
        let amount = (available as usize).min(buffer.len());
        let count = pipe.read(&mut buffer[..amount])?;
        ensure!(count > 0, "Original hold pipe ended unexpectedly.");
        bytes.extend_from_slice(&buffer[..count]);
        available -= count as u32;
    }
    Ok(false)
}

fn discard_available(pipe: &mut (impl AsRawHandle + Read)) -> Result<bool> {
    let mut available = 0;
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut available,
            ptr::null_mut(),
        )
    } == 0
    {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
            return Ok(true);
        }
        return Err(error.into());
    }
    let mut bytes = [0; 4096];
    for _ in 0..4 {
        let amount = (available as usize).min(bytes.len());
        if amount == 0 {
            break;
        }
        let count = pipe.read(&mut bytes[..amount])?;
        ensure!(count > 0, "Original hold drain ended unexpectedly.");
        available -= count as u32;
    }
    Ok(false)
}

pub(super) struct Hold {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    error: ChildStderr,
    bytes: Vec<u8>,
    errors: Vec<u8>,
    receipt: Value,
    output_eof: bool,
    error_eof: bool,
    finished: bool,
    original: Option<Original>,
}
impl Hold {
    pub(super) fn start(
        program: &Path,
        environment: &[(String, String)],
        receipt: &Value,
        image: &platform::FileIdentity,
        attached: bool,
        until: Instant,
    ) -> Result<Self> {
        let id = receipt["request_id"]
            .as_str()
            .context("Original rollback UUID unavailable.")?;
        ensure!(
            receipt["accepted"] == true
                && receipt["durable"] == true
                && receipt["revoked"] == true
                && receipt["checkpointed"] == true,
            "Original rollback is not settled."
        );
        let child = Command::new(program)
            .args(["desktop", "revoke-hold", "--json", "--request-id", id])
            .env_clear()
            .envs(environment.iter().map(|(k, v)| (k, v)))
            .creation_flags(if attached { 0 } else { CREATE_NO_WINDOW })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        Self::from_child(child, receipt, image, until)
    }
    fn from_child(
        mut child: Child,
        receipt: &Value,
        image: &platform::FileIdentity,
        until: Instant,
    ) -> Result<Self> {
        let input = child
            .stdin
            .take()
            .context("Original hold stdin unavailable.")?;
        let output = child
            .stdout
            .take()
            .context("Original hold stdout unavailable.")?;
        let error = child
            .stderr
            .take()
            .context("Original hold stderr unavailable.")?;
        let mut value = Self {
            child,
            input: Some(input),
            output,
            error,
            bytes: Vec::new(),
            errors: Vec::new(),
            receipt: receipt.clone(),
            output_eof: false,
            error_eof: false,
            finished: false,
            original: None,
        };
        // The mode helper alone owns this writer. Task Scheduler and every
        // other child must never inherit a second EOF-controlling reference.
        for raw in [
            value.input.as_ref().expect("writer").as_raw_handle(),
            value.output.as_raw_handle(),
            value.error.as_raw_handle(),
        ] {
            ensure!(
                unsafe { SetHandleInformation(raw, HANDLE_FLAG_INHERIT, 0) } != 0,
                "Original hold pipe inheritance could not be disabled."
            );
        }
        ensure!(
            value.child.try_wait()?.is_none(),
            "Original hold child already exited."
        );
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        ensure!(
            unsafe {
                GetProcessTimes(
                    value.child.as_raw_handle(),
                    &mut created,
                    &mut exited,
                    &mut kernel,
                    &mut user,
                )
            } != 0,
            "Original hold child birth unavailable."
        );
        let birth = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
        let original = Original::open(value.child.id())?;
        let (_, actual) = instances::selected_image(Path::new(&original.image))?;
        ensure!(
            original.birth == birth
                && original.parent == std::process::id()
                && original.own
                && original.current()
                && image.device == actual.device
                && image.inode == actual.inode
                && value.child.try_wait()?.is_none(),
            "Original hold child image or birth changed."
        );
        value.original = Some(original);
        loop {
            value.poll()?;
            if let Some(frame) = value.line()? {
                ensure!(
                    frame["abi"] == HOLD_ABI
                        && frame["held"] == true
                        && frame["receipt"] == value.receipt,
                    "Original hold readiness does not match the settled rollback."
                );
                value.check()?;
                return Ok(value);
            }
            ensure!(
                value.child.try_wait()?.is_none() && Instant::now() < until,
                "Original rollback exclusion is unavailable; keep the pending UUID."
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn poll(&mut self) -> Result<()> {
        if !self.output_eof {
            self.output_eof = read_available(&mut self.output, &mut self.bytes, 8192)?;
        }
        if !self.error_eof {
            self.error_eof = read_available(&mut self.error, &mut self.errors, 16384)?;
        }
        Ok(())
    }
    fn line(&mut self) -> Result<Option<Value>> {
        let Some(end) = self.bytes.iter().position(|b| *b == b'\n') else {
            return Ok(None);
        };
        let tail = self.bytes.split_off(end + 1);
        let frame = serde_json::from_slice(&self.bytes[..end])?;
        self.bytes = tail;
        Ok(Some(frame))
    }
    pub(super) fn check(&mut self) -> Result<()> {
        self.poll()?;
        ensure!(
            self.input.is_some()
                && self.child.try_wait()?.is_none()
                && self
                    .original
                    .as_ref()
                    .is_some_and(|original| original.current())
                && !self.output_eof
                && !self.error_eof
                && self.bytes.is_empty(),
            "Original rollback exclusion was lost; no later manager step is allowed."
        );
        Ok(())
    }
    pub(super) fn finish(&mut self, until: Instant) -> Result<()> {
        self.check()?;
        drop(self.input.take());
        let mut released = false;
        loop {
            self.poll()?;
            if let Some(frame) = self.line()? {
                ensure!(
                    !released
                        && frame["abi"] == HOLD_ABI
                        && frame["held"] == false
                        && frame["released"] == true
                        && frame["receipt"] == self.receipt,
                    "Original hold release receipt differs."
                );
                released = true;
            }
            if let Some(status) = self.child.try_wait()? {
                if self.output_eof && self.error_eof {
                    ensure!(
                        released && status.success() && self.bytes.is_empty(),
                        "Original hold release is unconfirmed."
                    );
                    self.finished = true;
                    return Ok(());
                }
            }
            ensure!(
                Instant::now() < until,
                "Original hold release is pending; retain the transition."
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Fenced abort also releases by original EOF. Keep the actual Child
        // and both readers until physical completion. The outer owned runner
        // bounds this worker; a logical timeout never advertises a clean join.
        drop(self.input.take());
        loop {
            // Protocol refusal must not prevent physical pipe drain. In
            // particular, an oversized frame cannot keep failing at the same
            // buffer bound while the original child waits for its pipe write.
            if !self.output_eof {
                self.output_eof = discard_available(&mut self.output).unwrap_or(false);
            }
            if !self.error_eof {
                self.error_eof = discard_available(&mut self.error).unwrap_or(false);
            }
            if self.child.try_wait().is_ok_and(|exit| exit.is_some())
                && self.output_eof
                && self.error_eof
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = "ZERUS_WINDOWS_HOLD_FIXTURE";
    const TEST: &str =
        "desktop::windows_registration::native::hold_child::tests::private_hold_child";

    #[test]
    fn private_hold_child() {
        let Ok(mode) = std::env::var(FIXTURE) else {
            return;
        };
        let receipt: Value =
            serde_json::from_str(&std::env::var("ZERUS_WINDOWS_HOLD_RECEIPT").unwrap()).unwrap();
        // libtest writes its preamble to stdout. The private parent swaps the
        // original stderr pipe into the protocol slot; no production switch.
        let stderr = std::io::stderr();
        let mut output = stderr.lock();
        if mode == "oversized" {
            output.write_all(&vec![b'x'; 9000]).unwrap();
            output.flush().unwrap();
        }
        writeln!(
            output,
            "{}",
            json!({"abi":HOLD_ABI,"held":true,"receipt":receipt})
        )
        .unwrap();
        output.flush().unwrap();
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes).unwrap();
        assert!(bytes.is_empty());
        if mode == "wrong_release" {
            writeln!(
                output,
                "{}",
                json!({"abi":HOLD_ABI,"held":false,"released":true,"receipt":{}})
            )
            .unwrap();
        } else {
            writeln!(
                output,
                "{}",
                json!({"abi":HOLD_ABI,"held":false,"released":true,"receipt":receipt})
            )
            .unwrap();
        }
        output.flush().unwrap();
        // Explicit parent release keeps the ORIGINAL process alive after the
        // logical deadline; scheduler timing cannot turn it into a clean exit.
        if mode == "late_exit" {
            let release =
                std::path::PathBuf::from(std::env::var_os("ZERUS_WINDOWS_HOLD_RELEASE").unwrap());
            let until = Instant::now() + Duration::from_secs(5);
            while !release.try_exists().unwrap() {
                assert!(
                    Instant::now() < until,
                    "private parent did not release fixture"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    fn fixture(mode: &str, release: Option<&Path>) -> (Child, platform::FileIdentity, Value) {
        use std::os::windows::io::OwnedHandle;
        let program = std::env::current_exe().unwrap();
        let (_image, id) = instances::selected_image(&program).unwrap();
        let receipt = json!({"request_id":uuid::Uuid::new_v4().to_string(),"configuration":"a".repeat(64),
            "writer_epoch":null,"accepted":true,"durable":true,"revoked":true,"checkpointed":true});
        let mut command = Command::new(program);
        command
            .args(["--exact", TEST, "--nocapture"])
            .env(FIXTURE, mode)
            .env("ZERUS_WINDOWS_HOLD_RECEIPT", receipt.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        if let Some(release) = release {
            command.env("ZERUS_WINDOWS_HOLD_RELEASE", release);
        }
        let mut child = command.spawn().unwrap();
        let logs: OwnedHandle = child.stdout.take().unwrap().into();
        let protocol: OwnedHandle = child.stderr.take().unwrap().into();
        child.stdout = Some(protocol.into());
        child.stderr = Some(logs.into());
        (child, id, receipt)
    }
    fn holder(mode: &str) -> Hold {
        let (child, id, receipt) = fixture(mode, None);
        Hold::from_child(
            child,
            &receipt,
            &id,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap()
    }

    #[test]
    fn original_eof_release_requires_the_matching_frame_exit_and_both_pipe_eofs() {
        let mut hold = holder("normal");
        hold.check().unwrap();
        let original = Original::open(hold.child.id()).unwrap();
        assert!(!original.exited());
        hold.finish(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert!(original.exited());
        assert!(hold.finished && hold.output_eof && hold.error_eof);
        let mut wrong = holder("wrong_release");
        assert!(wrong
            .finish(Instant::now() + Duration::from_secs(5))
            .is_err());
        assert!(!wrong.finished);
    }

    #[test]
    fn logical_release_expiry_keeps_original_child_and_pipe_custody_until_exit() {
        let root = tempfile::tempdir().unwrap();
        let release = root.path().join("release");
        let (child, id, receipt) = fixture("late_exit", Some(&release));
        let mut hold = Hold::from_child(
            child,
            &receipt,
            &id,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        let original = Original::open(hold.child.id()).unwrap();
        assert!(hold.finish(Instant::now()).is_err());
        assert!(!hold.finished);
        assert!(!original.exited());
        std::fs::write(release, b"done").unwrap();
        drop(hold);
        assert!(original.exited());
    }
    #[test]
    fn oversized_protocol_refuses_but_still_drains_the_original_child() {
        let (child, id, receipt) = fixture("oversized", None);
        let original = Original::open(child.id()).unwrap();
        assert!(Hold::from_child(
            child,
            &receipt,
            &id,
            Instant::now() + Duration::from_secs(5)
        )
        .is_err());
        assert!(original.exited());
    }
}
