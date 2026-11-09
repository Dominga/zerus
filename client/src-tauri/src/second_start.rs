//! A second start never exits silently (task 110).
//!
//! When another Zerus holds the single-instance channel and this start could
//! not hand its activation over — a Zerus that does not answer, or a pipe this
//! start may not open — it looks for that Zerus itself: same user, same Windows
//! session, an `hgs-tray.exe` image (or this build's own file name). An already
//! visible window of it is brought to the front with this process's own
//! foreground right; nothing is written to an unverified pipe and no right is
//! passed on. Otherwise a native message names what runs and how to switch. A
//! background start (login, `--background`) stays quiet, as before; the fork's
//! update restart is unchanged, because its helper waits for the old process to
//! exit.
use zerus_client_core::text::Text;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Running {
    pub pid: u32,
}

/// Why this start could not hand its activation over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Another Zerus holds the channel but did not answer (Claim::Duplicate).
    Unanswered,
    /// The channel could not be claimed (Claim::Failed), with the OS error.
    Failed(String),
    /// The pipe's server could not be verified (Windows Claim::Untrusted).
    Untrusted,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// A background start: the log has it; no window was asked for.
    Quiet,
    /// The running Zerus's window is in front now.
    Raised(u32),
    /// A native message: title, text and whether it reports an error.
    Message { title: Text, text: Text, error: bool },
    /// Nothing that is Zerus runs: the pipe belongs to another program.
    Untrusted,
}

/// The decision, apart from the native calls: `raise` tries one process's
/// visible window and says whether it is in front.
pub(crate) fn decide(
    refusal: &Refusal,
    asked: bool,
    running: &[Running],
    mut raise: impl FnMut(u32) -> bool,
) -> Step {
    if *refusal == Refusal::Untrusted && running.is_empty() {
        return Step::Untrusted;
    }
    if !asked {
        return Step::Quiet;
    }
    if let Some(found) = running.iter().find(|found| raise(found.pid)) {
        return Step::Raised(found.pid);
    }
    // Verbatim: a process number is a name here, not a count.
    let pid = |found: &Running| found.pid.to_string();
    match (running.first(), refusal) {
        (Some(found), _) => Step::Message {
            title: zerus_client_core::text!("Zerus is already running"),
            text: zerus_client_core::text!(
                "Zerus Next is already running as process %1 but did not answer this start. Open it from its icon in the notification area. If it does not respond, quit it from that icon's menu or end it in Task Manager, then start Zerus again.",
                pid(found)
            ),
            error: false,
        },
        (None, Refusal::Failed(error)) => Step::Message {
            title: zerus_client_core::text!("Zerus could not start"),
            text: zerus_client_core::text!(
                "Zerus could not claim its single-instance channel (%1), and no other Zerus answered. Close any other Zerus, then start Zerus again.",
                error.as_str()
            ),
            error: true,
        },
        (None, _) => Step::Message {
            title: zerus_client_core::text!("Zerus is already running"),
            text: zerus_client_core::text!(
                "Another Zerus is already running but did not answer this start. Open it from its icon in the notification area, or quit it, then start Zerus again."
            ),
            error: false,
        },
    }
}

/// Runs the decision with the native lookups and shows its message, if any.
/// `asked` is false for a background start.
pub(crate) fn settle(context: tauri::Context<tauri::Wry>, refusal: Refusal, asked: bool) {
    let running = running();
    let step = decide(&refusal, asked, &running, raise);
    let code = match &step {
        Step::Quiet => "second_start_quiet",
        Step::Raised(_) => "second_start_raised",
        Step::Message { .. } => "second_start_explained",
        Step::Untrusted => "second_start_untrusted",
    };
    crate::startup_log::record("duplicate", code, None);
    match step {
        Step::Quiet | Step::Raised(_) => {}
        Step::Message { title, text, error } => crate::startup_error::show_notice(
            context,
            crate::native_text::render(&title),
            crate::native_text::render(&text),
            error,
        ),
        Step::Untrusted => crate::startup_error::show_untrusted(context),
    }
}

#[cfg(not(windows))]
fn running() -> Vec<Running> {
    Vec::new()
}
#[cfg(not(windows))]
fn raise(_pid: u32) -> bool {
    false
}

#[cfg(windows)]
use native::{raise, running};

#[cfg(windows)]
mod native {
    use super::Running;
    use crate::platform::windows::{own_sid, owned, token_sid};
    use std::os::windows::io::{AsRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM},
        Security::{GetTokenInformation, TokenSessionId, TOKEN_QUERY},
        System::Diagnostics::ToolHelp::*,
        System::Threading::{
            GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::*,
    };

    /// At most this many other Zerus processes are looked at.
    const LIMIT: usize = 8;

    /// This user's other Zerus processes in this Windows session.
    pub(super) fn running() -> Vec<Running> {
        let mut names = vec!["hgs-tray.exe".to_owned()];
        if let Some(own) = std::env::current_exe()
            .ok()
            .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()))
        {
            if !names.contains(&own) {
                names.push(own);
            }
        }
        running_named(&names)
    }

    pub(super) fn running_named(names: &[String]) -> Vec<Running> {
        let own_pid = unsafe { GetCurrentProcessId() };
        let (Ok(own_user), Some(own_session)) = (own_sid(), session_of(own_pid)) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for (pid, name) in processes() {
            if found.len() >= LIMIT {
                break;
            }
            if pid == own_pid || !names.contains(&name.to_lowercase()) {
                continue;
            }
            if user_of(pid).as_deref() != Some(own_user.as_str())
                || session_of(pid) != Some(own_session)
            {
                continue;
            }
            found.push(Running { pid });
        }
        found
    }

    fn processes() -> Vec<(u32, String)> {
        let Ok(snapshot) = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return Vec::new();
        };
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut list = Vec::new();
        let mut more = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) } != 0;
        while more {
            list.push((entry.th32ProcessID, text(&entry.szExeFile)));
            more = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) } != 0;
        }
        list
    }

    fn token(pid: u32) -> Option<OwnedHandle> {
        let process = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) }).ok()?;
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut token) } == 0 {
            return None;
        }
        owned(token).ok()
    }
    fn user_of(pid: u32) -> Option<String> {
        token_sid(&token(pid)?).ok()
    }
    fn session_of(pid: u32) -> Option<u32> {
        let token = token(pid)?;
        let mut session = 0u32;
        let mut size = 0;
        (unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenSessionId,
                (&mut session as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
                &mut size,
            )
        } != 0)
            .then_some(session)
    }

    fn text(units: &[u16]) -> String {
        let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
        String::from_utf16_lossy(&units[..end])
    }

    struct Search {
        pid: u32,
        found: HWND,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> windows_sys::core::BOOL {
        let search = unsafe { &mut *(data as *mut Search) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == search.pid && main_window(hwnd) {
            search.found = hwnd;
            return 0;
        }
        1
    }
    /// A visible (possibly minimized), unowned, titled application window;
    /// tray helpers and hidden windows never count.
    fn main_window(hwnd: HWND) -> bool {
        unsafe {
            IsWindowVisible(hwnd) != 0
                && GetWindow(hwnd, GW_OWNER).is_null()
                && (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW) == 0
                && GetWindowTextLengthW(hwnd) > 0
        }
    }
    pub(super) fn window_of(pid: u32) -> Option<HWND> {
        let mut search = Search {
            pid,
            found: std::ptr::null_mut(),
        };
        unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
        (!search.found.is_null()).then_some(search.found)
    }

    /// Only an already visible window comes forward; a Zerus that runs in
    /// the tray alone gets the message instead.
    pub(super) fn raise(pid: u32) -> bool {
        use crate::windows_shell::Front;
        window_of(pid).is_some_and(|hwnd| {
            matches!(
                crate::platform::windows::foreground::present_handle(hwnd),
                Front::Already | Front::Taken
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn english(text: &Text) -> String {
        crate::native_text::render(text)
    }
    fn one(pid: u32) -> Running {
        Running { pid }
    }

    #[test]
    fn a_visible_window_of_the_running_zerus_comes_forward_instead_of_a_message() {
        let mut tried = Vec::new();
        let step = decide(&Refusal::Unanswered, true, &[one(40), one(41)], |pid| {
            tried.push(pid);
            pid == 41
        });
        assert_eq!(step, Step::Raised(41));
        assert_eq!(tried, [40, 41]);
    }

    #[test]
    fn the_message_names_the_running_zerus_and_how_to_start_it() {
        let Step::Message { title, text, error } =
            decide(&Refusal::Unanswered, true, &[one(1234)], |_| false)
        else {
            panic!("a message")
        };
        assert!(!error);
        assert_eq!(english(&title), "Zerus is already running");
        let text = english(&text);
        assert!(text.contains("process 1234"), "{text}");
        assert!(!text.contains("Qt"), "{text}");

        let Step::Message { text, .. } =
            decide(&Refusal::Failed("Access is denied.".into()), true, &[one(77)], |_| false)
        else {
            panic!("a message")
        };
        let text = english(&text);
        assert!(text.starts_with("Zerus Next is already running as process 77"), "{text}");
    }

    #[test]
    fn nothing_running_says_why_the_start_failed_or_that_zerus_did_not_answer() {
        let Step::Message { title, text, error } = decide(
            &Refusal::Failed("Access is denied. (os error 5)".into()),
            true,
            &[],
            |_| unreachable!(),
        ) else {
            panic!("a message")
        };
        assert!(error);
        assert_eq!(english(&title), "Zerus could not start");
        assert!(english(&text).contains("(Access is denied. (os error 5))"));
        let Step::Message { error, .. } = decide(&Refusal::Unanswered, true, &[], |_| false) else {
            panic!("a message")
        };
        assert!(!error);
    }

    #[test]
    fn a_background_start_stays_quiet_and_an_unknown_pipe_owner_keeps_its_warning() {
        let never = |_| -> bool { panic!("a background start raises nothing") };
        assert_eq!(decide(&Refusal::Unanswered, false, &[one(1)], never), Step::Quiet);
        assert_eq!(
            decide(&Refusal::Failed("x".into()), false, &[], never),
            Step::Quiet
        );
        // No Zerus runs: the pipe is another program's, whatever was asked.
        assert_eq!(decide(&Refusal::Untrusted, false, &[], never), Step::Untrusted);
        assert_eq!(decide(&Refusal::Untrusted, true, &[], never), Step::Untrusted);
        // A running Zerus behind an unverifiable pipe is raised, not warned at.
        assert_eq!(
            decide(&Refusal::Untrusted, true, &[one(9)], |pid| pid == 9),
            Step::Raised(9)
        );
    }

    #[test]
    fn messages_never_separate_values_with_dots() {
        for running in [vec![one(1)], vec![one(2), one(3)], vec![]] {
            for refusal in [Refusal::Unanswered, Refusal::Failed("e".into())] {
                if let Step::Message { text, .. } = decide(&refusal, true, &running, |_| false) {
                    let text = english(&text);
                    assert!(!text.contains('\u{00b7}') && !text.contains('\u{2022}'), "{text}");
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn running_finds_this_users_process_by_image_name_and_reads_its_modules() {
        let mut child = std::process::Command::new(
            std::path::Path::new(&std::env::var_os("SystemRoot").unwrap())
                .join("System32")
                .join("ping.exe"),
        )
        .args(["-n", "30", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
        let pid = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let found = loop {
            let found = native::running_named(&["ping.exe".to_owned()]);
            if found.iter().any(|running| running.pid == pid)
                || std::time::Instant::now() >= deadline
            {
                break found;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let _ = child.kill();
        let _ = child.wait();
        assert!(found.iter().any(|running| running.pid == pid), "the child is found");
        let own = std::process::id();
        assert!(native::running_named(&["ping.exe".to_owned()])
            .iter()
            .all(|running| running.pid != own));
        // ping has no window: nothing would come forward.
        assert!(native::window_of(pid).is_none());
    }
}
