//! The Terminal's attach client in a Windows pseudoconsole (ConPTY), the
//! Windows side of [`crate::pty`] with the same contract: output in reads of
//! at most 256 KiB handed on through the consumer's bounded channel, input
//! queued up to 4 MiB, the last 1 MiB kept for a view shown again, and one
//! finish with the client's exit code. Stop closes the pseudoconsole, which
//! ends only the clients attached to it (`hgs a`, ssh, the tmux client); a
//! session host, tmux server or agent never belongs to it.
//!
//! Three threads per attach: the reader drains the output pipe (also while
//! a stop closes the console, which waits for it to drain), the writer
//! sends queued input, and the waiter closes the console when the client
//! exits so the reader sees the end.
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, UpdateProcThreadAttribute, WaitForSingleObject,
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, INFINITE, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::terminal::AttachEvent as Event;

/// Bytes read per round before the output is handed on.
pub(crate) const MAX_READ: usize = 256 * 1024;
/// Queued terminal input.
pub(crate) const MAX_QUEUED_INPUT: usize = 4 * 1024 * 1024;
/// Output kept for a view that is shown again.
pub(crate) const REPLAY_BYTES: usize = 1024 * 1024;
/// How often a full channel is retried while a stop may be under way.
const RETRY: Duration = Duration::from_millis(20);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A kernel handle owned by one of the attach's threads.
#[derive(Clone, Copy)]
struct Handle(isize);

// SAFETY: a Win32 handle is a process-wide value; each is used and closed
// by one owner at a time.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    fn raw(self) -> HANDLE {
        self.0 as HANDLE
    }

    fn close(self) {
        // SAFETY: closed once, by its owner.
        unsafe {
            CloseHandle(self.raw());
        }
    }
}

/// The size a console gets: 2-1000 columns and 2-500 rows.
pub(crate) fn size(cols: u16, rows: u16) -> COORD {
    COORD {
        X: cols.clamp(2, 1000) as i16,
        Y: rows.clamp(2, 500) as i16,
    }
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

/// One argument quoted as the C runtime parses a command line.
fn quote(argument: &OsStr) -> Vec<u16> {
    let units: Vec<u16> = argument.encode_wide().collect();
    let plain = !units.is_empty()
        && !units
            .iter()
            .any(|unit| matches!(*unit, 0x20 | 0x09 | 0x0a | 0x0b | 0x22));
    if plain {
        return units;
    }
    let mut quoted = vec![u16::from(b'"')];
    let mut backslashes = 0;
    for unit in units {
        if unit == u16::from(b'\\') {
            backslashes += 1;
            continue;
        }
        let doubled = if unit == u16::from(b'"') {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        quoted.extend(std::iter::repeat_n(u16::from(b'\\'), doubled));
        quoted.push(unit);
        backslashes = 0;
    }
    quoted.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes * 2));
    quoted.push(u16::from(b'"'));
    quoted
}

/// The command line: the program and its arguments, each quoted.
pub(crate) fn command_line(program: &Path, args: &[String]) -> Vec<u16> {
    let mut line = quote(program.as_os_str());
    for argument in args {
        line.push(u16::from(b' '));
        line.extend(quote(OsStr::new(argument)));
    }
    line.push(0);
    line
}

/// The environment block: `KEY=VALUE` entries sorted ignoring case, each
/// ending in NUL, then one more NUL.
pub(crate) fn environment_block(env: &[(OsString, OsString)]) -> Vec<u16> {
    let mut entries: Vec<&(OsString, OsString)> = env.iter().collect();
    entries.sort_by_key(|(key, _)| key.to_string_lossy().to_uppercase());
    let mut block = Vec::new();
    for (key, value) in entries {
        block.extend(key.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// `program` itself when absolute, otherwise the first match on `PATH`, also
/// with each `PATHEXT` extension when it has none.
fn resolve(program: &Path) -> Option<PathBuf> {
    let file = |path: &Path| path.is_file().then(|| path.to_path_buf());
    if program.is_absolute() {
        return file(program);
    }
    let extensions: Vec<String> = if program.extension().is_some() {
        vec![String::new()]
    } else {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(str::to_owned)
            .collect()
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|folder| {
        extensions.iter().find_map(|extension| {
            let mut name = program.as_os_str().to_owned();
            name.push(extension);
            file(&folder.join(name))
        })
    })
}

struct Shared {
    console: Mutex<Option<HPCON>>,
    process: Mutex<Option<Handle>>,
    pid: Mutex<Option<u32>>,
    exit_code: Mutex<Option<u32>>,
    pending: Mutex<Vec<u8>>,
    input_ready: Condvar,
    replay: Mutex<VecDeque<u8>>,
    stopped: AtomicBool,
    ended: AtomicBool,
}

impl Shared {
    /// Closes the console once: its clients end, then its output ends.
    fn close_console(&self) {
        if let Some(console) = lock(&self.console).take() {
            // SAFETY: created by this attach and closed only here.
            unsafe { ClosePseudoConsole(console) };
        }
    }
}

/// A running attach client in its pseudoconsole.
pub(crate) struct Pty {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

struct Pipes {
    input_read: Handle,
    input_write: Handle,
    output_read: Handle,
    output_write: Handle,
}

fn pipes() -> Result<Pipes, String> {
    let mut handles: [HANDLE; 4] = [std::ptr::null_mut(); 4];
    // SAFETY: plain pipe creation into local handles.
    let created = unsafe {
        CreatePipe(&mut handles[0], &mut handles[1], std::ptr::null(), 0) != 0
            && CreatePipe(&mut handles[2], &mut handles[3], std::ptr::null(), 0) != 0
    };
    if !created {
        for handle in handles.into_iter().filter(|handle| !handle.is_null()) {
            Handle(handle as isize).close();
        }
        return Err(format!(
            "Cannot open terminal: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(Pipes {
        input_read: Handle(handles[0] as isize),
        input_write: Handle(handles[1] as isize),
        output_read: Handle(handles[2] as isize),
        output_write: Handle(handles[3] as isize),
    })
}

impl Pty {
    /// Start `program` with `args` and exactly `env` in a new pseudoconsole
    /// of `cols` x `rows`.
    pub(crate) fn start(
        program: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
        cols: u16,
        rows: u16,
        events: SyncSender<Event>,
    ) -> Result<Pty, String> {
        let program = resolve(program)
            .ok_or_else(|| "Cannot open terminal: the hgs executable was not found".to_owned())?;
        let pipes = pipes()?;
        let mut console: HPCON = 0;
        // SAFETY: the pipe ends are open; ConPTY duplicates what it keeps.
        let result = unsafe {
            CreatePseudoConsole(
                size(cols, rows),
                pipes.input_read.raw(),
                pipes.output_write.raw(),
                0,
                &mut console,
            )
        };
        // The console holds its own copies of these ends.
        pipes.input_read.close();
        pipes.output_write.close();
        if result < 0 {
            pipes.input_write.close();
            pipes.output_read.close();
            return Err(format!("Cannot open terminal: error {result:#x}"));
        }
        let started = spawn(&program, args, env, console);
        let (process, pid) = match started {
            Ok(started) => started,
            Err(error) => {
                // SAFETY: the console has no client yet.
                unsafe { ClosePseudoConsole(console) };
                pipes.input_write.close();
                pipes.output_read.close();
                return Err(error);
            }
        };
        let shared = Arc::new(Shared {
            console: Mutex::new(Some(console)),
            process: Mutex::new(Some(process)),
            pid: Mutex::new(Some(pid)),
            exit_code: Mutex::new(None),
            pending: Mutex::new(Vec::new()),
            input_ready: Condvar::new(),
            replay: Mutex::new(VecDeque::new()),
            stopped: AtomicBool::new(false),
            ended: AtomicBool::new(false),
        });
        let mut threads = Vec::new();
        let names = [
            "zerus-terminal-read",
            "zerus-terminal-write",
            "zerus-terminal-wait",
        ];
        let reader = Arc::clone(&shared);
        let writer = Arc::clone(&shared);
        let waiter = Arc::clone(&shared);
        let output = pipes.output_read;
        let input = pipes.input_write;
        let bodies: [Box<dyn FnOnce() + Send>; 3] = [
            Box::new(move || read(&reader, output, &events)),
            Box::new(move || write(&writer, input)),
            Box::new(move || wait(&waiter, process)),
        ];
        for (name, body) in names.into_iter().zip(bodies) {
            match std::thread::Builder::new().name(name.into()).spawn(body) {
                Ok(thread) => threads.push(thread),
                Err(error) => {
                    let mut pty = Pty { shared, threads };
                    pty.stop();
                    return Err(format!("Cannot open terminal: {error}"));
                }
            }
        }
        Ok(Pty { shared, threads })
    }

    pub(crate) fn pid(&self) -> Option<u32> {
        *lock(&self.shared.pid)
    }

    /// Queue input. Over 4 MiB pending it fails, and nothing is queued.
    pub(crate) fn write(&self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || self.shared.ended.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut pending = lock(&self.shared.pending);
        if bytes.len() > MAX_QUEUED_INPUT - pending.len() {
            return Err("Terminal input queue is full; wait before pasting more text.".into());
        }
        pending.extend_from_slice(bytes);
        self.shared.input_ready.notify_one();
        Ok(())
    }

    pub(crate) fn resize(&self, cols: u16, rows: u16) {
        if let Some(console) = *lock(&self.shared.console) {
            // SAFETY: the console stays open while its lock is held.
            unsafe {
                ResizePseudoConsole(console, size(cols, rows));
            }
        }
    }

    pub(crate) fn replay(&self) -> Vec<u8> {
        lock(&self.shared.replay).iter().copied().collect()
    }

    /// Close the console, ending its clients, and wait for the threads.
    /// Nothing is reported after a stop.
    pub(crate) fn stop(&mut self) {
        if self.shared.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        self.shared.ended.store(true, Ordering::SeqCst);
        self.shared.input_ready.notify_all();
        // The reader keeps draining while the console closes.
        self.shared.close_console();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Creates the client in the console: no inherited handles and no standard
/// handles of this process, so it reads and writes only the console.
fn spawn(
    program: &Path,
    args: &[String],
    env: &[(OsString, OsString)],
    console: HPCON,
) -> Result<(Handle, u32), String> {
    let mut size = 0usize;
    // SAFETY: the first call only reports the list's size.
    unsafe {
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
    }
    let mut list = vec![0u8; size];
    let attributes = list.as_mut_ptr().cast();
    // SAFETY: `list` has the size the system asked for and outlives its use.
    let ready = unsafe {
        InitializeProcThreadAttributeList(attributes, 1, 0, &mut size) != 0
            && UpdateProcThreadAttribute(
                attributes,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *const std::ffi::c_void,
                std::mem::size_of::<HPCON>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            ) != 0
    };
    if !ready {
        return Err(format!(
            "Cannot open terminal: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: an all-zero STARTUPINFOEXW is its documented empty value.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attributes;
    let application = wide(program.as_os_str());
    let mut line = command_line(program, args);
    let block = environment_block(env);
    // SAFETY: an all-zero PROCESS_INFORMATION is filled by the call.
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: every buffer is NUL-terminated and alive for the call.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            block.as_ptr().cast(),
            std::ptr::null(),
            &startup.StartupInfo,
            &mut process,
        ) != 0
    };
    let error = std::io::Error::last_os_error();
    // SAFETY: initialised above and no longer needed.
    unsafe { DeleteProcThreadAttributeList(attributes) };
    if !created {
        return Err(format!("Cannot open terminal: {error}"));
    }
    Handle(process.hThread as isize).close();
    Ok((Handle(process.hProcess as isize), process.dwProcessId))
}

/// Delivers an event, waiting while the consumer's channel is full; gives
/// up when a stop came, so a stop never waits for the consumer.
fn deliver(shared: &Shared, events: &SyncSender<Event>, mut event: Event) -> bool {
    loop {
        if shared.stopped.load(Ordering::SeqCst) {
            return false;
        }
        match events.try_send(event) {
            Ok(()) => return true,
            Err(TrySendError::Full(back)) => {
                event = back;
                std::thread::sleep(RETRY);
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

fn read(shared: &Shared, output: Handle, events: &SyncSender<Event>) {
    let mut buffer = vec![0u8; MAX_READ];
    loop {
        let mut count = 0u32;
        // SAFETY: the buffer is MAX_READ long and the handle ours.
        let ok = unsafe {
            ReadFile(
                output.raw(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut count,
                std::ptr::null_mut(),
            ) != 0
        };
        if !ok || count == 0 {
            break;
        }
        let data = buffer[..count as usize].to_vec();
        {
            let mut replay = lock(&shared.replay);
            replay.extend(&data);
            let excess = replay.len().saturating_sub(REPLAY_BYTES);
            replay.drain(..excess);
        }
        // After a stop the output is still drained, but no longer handed on.
        if !shared.stopped.load(Ordering::SeqCst) {
            deliver(shared, events, Event::Output { data });
        }
    }
    output.close();
    shared.ended.store(true, Ordering::SeqCst);
    shared.input_ready.notify_all();
    if !shared.stopped.load(Ordering::SeqCst) {
        let code = lock(&shared.exit_code).unwrap_or(1);
        deliver(shared, events, Event::Finished { code: code as i32 });
    }
}

fn write(shared: &Shared, input: Handle) {
    loop {
        let bytes = {
            let mut pending = lock(&shared.pending);
            while pending.is_empty() && !shared.ended.load(Ordering::SeqCst) {
                pending = shared
                    .input_ready
                    .wait(pending)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if shared.ended.load(Ordering::SeqCst) {
                break;
            }
            std::mem::take(&mut *pending)
        };
        let mut sent = 0;
        while sent < bytes.len() {
            let mut count = 0u32;
            // SAFETY: the slice is alive for the call and the handle ours.
            let ok = unsafe {
                WriteFile(
                    input.raw(),
                    bytes[sent..].as_ptr(),
                    (bytes.len() - sent) as u32,
                    &mut count,
                    std::ptr::null_mut(),
                ) != 0
            };
            if !ok || count == 0 {
                input.close();
                return;
            }
            sent += count as usize;
        }
    }
    input.close();
}

fn wait(shared: &Shared, process: Handle) {
    // SAFETY: the process handle stays open until this thread closes it.
    let ended = unsafe { WaitForSingleObject(process.raw(), INFINITE) } == WAIT_OBJECT_0;
    if ended {
        let mut code = 0u32;
        // SAFETY: the process ended; its code is final.
        if unsafe { GetExitCodeProcess(process.raw(), &mut code) } != 0 {
            *lock(&shared.exit_code) = Some(code);
        }
    }
    *lock(&shared.pid) = None;
    lock(&shared.process).take();
    process.close();
    // The client ended: closing the console ends its output, so the reader
    // reports the finish.
    shared.close_console();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;

    #[test]
    fn arguments_are_quoted_as_the_c_runtime_reads_them() {
        let text = |units: Vec<u16>| String::from_utf16(&units).unwrap();
        assert_eq!(text(quote(OsStr::new("plain"))), "plain");
        assert_eq!(text(quote(OsStr::new("two words"))), "\"two words\"");
        assert_eq!(text(quote(OsStr::new(""))), "\"\"");
        assert_eq!(text(quote(OsStr::new("say \"hi\""))), "\"say \\\"hi\\\"\"");
        assert_eq!(text(quote(OsStr::new("C:\\a b\\"))), "\"C:\\a b\\\\\"");
        let line = command_line(
            Path::new("C:\\hgs\\hgs.exe"),
            &["a".into(), "claude/p/my fix".into()],
        );
        assert_eq!(
            text(line[..line.len() - 1].to_vec()),
            "C:\\hgs\\hgs.exe a \"claude/p/my fix\""
        );
    }

    #[test]
    fn the_environment_block_is_sorted_and_double_terminated() {
        let block = environment_block(&[("b".into(), "2".into()), ("A".into(), "1".into())]);
        let text = String::from_utf16(&block).unwrap();
        assert_eq!(text, "A=1\0b=2\0\0");
        assert_eq!(environment_block(&[]), [0, 0]);
    }

    #[test]
    fn a_command_runs_in_a_pseudoconsole() {
        let (sender, events) = sync_channel(8);
        let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        let mut pty = Pty::start(
            Path::new("cmd.exe"),
            &["/c".into(), "echo zerus-conpty".into()],
            &env,
            80,
            24,
            sender,
        )
        .unwrap();
        let mut output = Vec::new();
        let code = loop {
            match events
                .recv_timeout(Duration::from_secs(20))
                .expect("an event")
            {
                Event::Output { data } => output.extend(data),
                Event::Finished { code } => break code,
            }
        };
        assert_eq!(code, 0);
        assert!(String::from_utf8_lossy(&output).contains("zerus-conpty"));
        pty.stop();
    }
}
