//! The console window of a launch on Windows when Windows Terminal is missing
//! or would change an argument (`tray/src/ConsoleHost.h`). The client started
//! as
//!
//! ```text
//! zerus --console-run --title TITLE -- PROGRAM ARGUMENTS...
//! ```
//!
//! opens a console of its own and starts the program with exactly these
//! arguments, with no shell parsing them. When the program fails, the window
//! stays open with its exit code until Enter is pressed. The client's `main`
//! hands these arguments to [`run`] before anything else starts.

/// The option that makes the client a console host.
pub const OPTION: &str = "--console-run";

/// A console window's request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub title: String,
    pub program: String,
    pub arguments: Vec<String>,
}

/// The client's arguments (after the program name) that run `program` in a
/// console window.
pub fn arguments(title: &str, program: &str, arguments: &[String]) -> Vec<String> {
    let mut result = vec![
        OPTION.to_owned(),
        "--title".to_owned(),
        title.to_owned(),
        "--".to_owned(),
        program.to_owned(),
    ];
    result.extend_from_slice(arguments);
    result
}

/// The request in the client's arguments after the program name; None when
/// they are not a well-formed [`OPTION`] request.
pub fn parse(arguments: &[String]) -> Option<Request> {
    if arguments.len() < 5
        || arguments[0] != OPTION
        || arguments[1] != "--title"
        || arguments[3] != "--"
        || arguments[4].is_empty()
    {
        return None;
    }
    Some(Request {
        title: arguments[2].clone(),
        program: arguments[4].clone(),
        arguments: arguments[5..].to_vec(),
    })
}

/// The line shown when the program failed: its name without `.exe`, and a
/// status code such as 0xC000013A (console closed) in hex.
pub fn failure_message(program: &str, exit_code: u32) -> String {
    let start = program.rfind(['/', '\\']).map_or(0, |index| index + 1);
    let mut name = &program[start..];
    let cut = name.len().saturating_sub(4);
    if name
        .get(cut..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(".exe"))
    {
        name = &name[..cut];
    }
    let code = if exit_code > 0xffff {
        format!("0x{exit_code:X}")
    } else {
        exit_code.to_string()
    };
    let name = if name.is_empty() { program } else { name };
    format!("{name} exited with code {code}. Press Enter to close this window.")
}

/// The reason a program did not start, as the console host shows it: the
/// system's English text, trimmed, or `error CODE` without one.
pub fn start_error(code: u32, message: Option<&str>) -> String {
    match message {
        Some(text) if !text.is_empty() => text.trim().to_owned(),
        _ => format!("error {code}"),
    }
}

/// Runs the request in `arguments` (after the program name) and returns the
/// program's exit code: 2 for a malformed request, 127 when it cannot start.
#[cfg(windows)]
pub fn run(arguments: &[String]) -> i32 {
    windows::run(arguments)
}

#[cfg(windows)]
mod windows {
    use super::super::system::{format_message, LANGUAGE_ENGLISH};
    use super::{failure_message, parse, start_error};
    use crate::paths::{self, Style};
    use crate::shell::windows_program_line;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_OPERATION_ABORTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE, TRUE,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        AllocConsole, FlushConsoleInputBuffer, ReadConsoleW, SetConsoleCtrlHandler, SetConsoleMode,
        SetConsoleTitleW, WriteConsoleW, CTRL_BREAK_EVENT, CTRL_C_EVENT, ENABLE_ECHO_INPUT,
        ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
        InitializeProcThreadAttributeList, Sleep, UpdateProcThreadAttribute, WaitForSingleObject,
        EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Ctrl+C and Ctrl+Break belong to the program, which shares this console;
    /// closing the window ends both.
    unsafe extern "system" fn keep_waiting(event: u32) -> i32 {
        (event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT) as i32
    }

    fn write(output: HANDLE, text: &str) {
        let text: Vec<u16> = text.encode_utf16().collect();
        let mut written = 0;
        // SAFETY: the buffer outlives the call and its length is exact.
        unsafe {
            WriteConsoleW(
                output,
                text.as_ptr().cast(),
                text.len() as u32,
                &mut written,
                null(),
            );
        }
    }

    fn wait_for_enter(input: HANDLE) {
        let mut buffer = [0u16; 256];
        // SAFETY: plain console calls on a console input handle this process
        // opened; the buffer length is exact.
        unsafe {
            // The program may have left the console in raw mode.
            SetConsoleMode(
                input,
                ENABLE_PROCESSED_INPUT | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT,
            );
            FlushConsoleInputBuffer(input);
            loop {
                let mut count = 0;
                let read = ReadConsoleW(
                    input,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut count,
                    null(),
                );
                if read == 0 {
                    // Ctrl+C ends a read without ending the wait.
                    if GetLastError() == ERROR_OPERATION_ABORTED {
                        continue;
                    }
                    return;
                }
                let read = &buffer[..count as usize];
                if read.contains(&u16::from(b'\r')) || read.contains(&u16::from(b'\n')) {
                    return;
                }
                if count == 0 {
                    Sleep(10);
                }
            }
        }
    }

    fn open_console(name: &str) -> HANDLE {
        // Inheritable: the program receives exactly these as its standard handles.
        let inheritable = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: TRUE,
        };
        let name = wide(name);
        // SAFETY: the name is NUL-terminated and the attributes live across the call.
        unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &inheritable,
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        }
    }

    pub(super) fn run(arguments: &[String]) -> i32 {
        let Some(request) = parse(arguments) else {
            return 2;
        };
        let title = wide(&request.title);
        // SAFETY: the title is NUL-terminated; the handler is a plain function.
        unsafe {
            // A GUI program has no console; this one is the window. It fails
            // only when a console is attached already, which serves as well.
            AllocConsole();
            SetConsoleTitleW(title.as_ptr());
            SetConsoleCtrlHandler(Some(keep_waiting), TRUE);
        }
        let input = open_console("CONIN$");
        let output = open_console("CONOUT$");
        if input == INVALID_HANDLE_VALUE || output == INVALID_HANDLE_VALUE {
            return 1;
        }

        // SAFETY: the attribute list lives in `storage` until it is deleted
        // below, the handle array outlives CreateProcessW, and every pointer
        // passed is valid for the call.
        unsafe {
            // Only the console handles are inherited, whatever else this
            // process holds.
            let mut size = 0;
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
            let mut storage = vec![0u8; size];
            let attributes: LPPROC_THREAD_ATTRIBUTE_LIST = storage.as_mut_ptr().cast();
            let inherited = [input, output];
            let listed = InitializeProcThreadAttributeList(attributes, 1, 0, &mut size) != 0;
            let limited = listed
                && UpdateProcThreadAttribute(
                    attributes,
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    inherited.as_ptr().cast(),
                    size_of_val(&inherited),
                    null_mut(),
                    null(),
                ) != 0;
            let mut startup: STARTUPINFOEXW = std::mem::zeroed();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = input;
            startup.StartupInfo.hStdOutput = output;
            startup.StartupInfo.hStdError = output;
            startup.lpAttributeList = if limited { attributes } else { null_mut() };

            // The arguments reach the program as QProcess would pass them:
            // nothing between this process and the program parses them.
            let program = request.program.replace('/', "\\");
            let application = wide(&program);
            let mut line = wide(&windows_program_line(&request.program, &request.arguments));
            let mut info: PROCESS_INFORMATION = std::mem::zeroed();
            let started = CreateProcessW(
                if paths::absolute(&request.program, Style::Windows) {
                    application.as_ptr()
                } else {
                    null()
                },
                line.as_mut_ptr(),
                null(),
                null(),
                TRUE,
                if limited {
                    EXTENDED_STARTUPINFO_PRESENT
                } else {
                    0
                },
                null(),
                null(),
                &startup.StartupInfo,
                &mut info,
            );
            let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32;
            if listed {
                DeleteProcThreadAttributeList(attributes);
            }
            if started == 0 {
                let reason = start_error(code, format_message(code, LANGUAGE_ENGLISH).as_deref());
                write(
                    output,
                    &format!(
                        "Cannot start {program}: {reason}\r\n\r\nPress Enter to close this window."
                    ),
                );
                wait_for_enter(input);
                return 127;
            }
            CloseHandle(info.hThread);
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut exit_code = 1;
            GetExitCodeProcess(info.hProcess, &mut exit_code);
            CloseHandle(info.hProcess);
            if exit_code != 0 {
                write(
                    output,
                    &format!("\r\n{}", failure_message(&request.program, exit_code)),
                );
                wait_for_enter(input);
            }
            exit_code as i32
        }
    }
}
