//! The terminal launcher against the Qt GUI's answers: the pure builders with
//! the cases of `tray/tests/test_terminalbackend.cpp`, `test_macbackend.cpp`
//! and `test_windowsdesktop.cpp`, the template splitter with a table recorded
//! from Qt 6.12's `QProcess::splitCommand`, and the backends' decisions with a
//! recorded desktop and Konsole.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zerus_client_core::shell::{self, Shell};
use zerus_client_core::terminal_launch::{
    self as launch, console_host, Backend, Desktop, Konsole, KonsoleMode, Launcher, Opened,
    Platform, Settings, Start, Window, WindowsMode,
};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// `QProcess::splitCommand` of each input, recorded from Qt 6.12.
const SPLIT: &[(&str, &[&str])] = &[
    (
        "foot -e bash -lc {cmd}",
        &["foot", "-e", "bash", "-lc", "{cmd}"],
    ),
    (
        "xterm -e sh -c \"{cmd}\"",
        &["xterm", "-e", "sh", "-c", "{cmd}"],
    ),
    ("  a   b  ", &["a", "b"]),
    ("\"a b\" c", &["a b", "c"]),
    ("a\"b c\"d", &["ab cd"]),
    ("\"\"", &[]),
    ("a \"\" b", &["a", "b"]),
    ("\"\"\"", &["\""]),
    ("a\"\"\"b", &["a\"b"]),
    ("\"a\"\"\"b\"", &["a\"b"]),
    ("x \"unterminated y", &["x", "unterminated y"]),
    ("tab\there", &["tab", "here"]),
    ("\"a\"\"b\"", &["ab"]),
    ("\"\"\"\"", &["\""]),
    ("a\"\"\"\"b", &["a\"b"]),
    ("wt.exe -w 0 \"{cmd}\"", &["wt.exe", "-w", "0", "{cmd}"]),
    ("say\u{a0}nbsp", &["say", "nbsp"]),
    ("line\nbreak x", &["line", "break", "x"]),
    ("\"\"\"\"\"\"", &["\"\""]),
    ("end\"", &["end"]),
];

#[test]
fn templates_split_as_qt_splits_them() {
    for (input, expected) in SPLIT {
        assert_eq!(launch::split_command(input), strings(expected), "{input:?}");
    }
}

#[test]
fn the_command_stays_one_argument() {
    assert_eq!(
        launch::build_argv("foot -e bash -lc {cmd}", "hgs a claude/x").unwrap(),
        strings(&["foot", "-e", "bash", "-lc", "hgs a claude/x"])
    );
    let argv =
        launch::build_argv("xterm -e sh -c \"{cmd}\"", "hgs a 'claude/my project/tag'").unwrap();
    assert_eq!(argv.len(), 5);
    assert_eq!(argv[4], "hgs a 'claude/my project/tag'");
    assert_eq!(
        launch::build_argv("alacritty --command={cmd}", "hgs a x").unwrap()[1],
        "--command=hgs a x"
    );
    assert!(launch::build_argv("", "hgs a x")
        .unwrap_err()
        .contains("{cmd}"));
    assert!(launch::build_argv("konsole -e bash", "hgs a x")
        .unwrap_err()
        .contains("{cmd}"));
    assert!(launch::build_argv("\"\"", "hgs a x")
        .unwrap_err()
        .contains("does not parse"));
}

#[test]
fn the_desktop_terminal_prefers_xdg_terminal_exec() {
    assert_eq!(
        launch::system_terminal_template(&|_| true),
        Some("xdg-terminal-exec bash -lc {cmd}")
    );
    let debian = |program: &str| program == "x-terminal-emulator";
    let template = launch::system_terminal_template(&debian).unwrap();
    assert_eq!(template, "x-terminal-emulator -e bash -lc {cmd}");
    assert_eq!(
        launch::build_argv(template, "hgs a 'claude/my project/tag'").unwrap(),
        strings(&[
            "x-terminal-emulator",
            "-e",
            "bash",
            "-lc",
            "hgs a 'claude/my project/tag'"
        ])
    );
    assert_eq!(launch::system_terminal_template(&|_| false), None);
}

#[test]
fn konsole_windows_start_a_login_shell() {
    assert_eq!(
        launch::konsole_launch_args("hgs a x"),
        strings(&["-e", "bash", "-lc", "hgs a x"])
    );
}

#[test]
fn pts_device_numbers_decode() {
    assert_eq!(
        launch::tty_from_dev_number((136 << 8) | 4).as_deref(),
        Some("/dev/pts/4")
    );
    assert_eq!(
        launch::tty_from_dev_number(136 << 8).as_deref(),
        Some("/dev/pts/0")
    );
    assert_eq!(
        launch::tty_from_dev_number((137 << 8) | 1).as_deref(),
        Some("/dev/pts/257")
    );
    assert_eq!(
        launch::tty_from_dev_number((143 << 8) | 255).as_deref(),
        Some("/dev/pts/2047")
    );
    for other in [0, (4 << 8) | 1, 5 << 8, 144 << 8] {
        assert_eq!(launch::tty_from_dev_number(other), None, "{other}");
    }
    assert_eq!(
        launch::normalize_tty("pts/4").as_deref(),
        Some("/dev/pts/4")
    );
    assert_eq!(
        launch::normalize_tty("/dev/pts/4").as_deref(),
        Some("/dev/pts/4")
    );
    assert_eq!(launch::normalize_tty("  "), None);
    assert_eq!(launch::tty_of_pid(0), None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_process_tty_matches_proc() {
    let expected = std::fs::read_link("/proc/self/fd/0")
        .ok()
        .and_then(|link| link.to_str().map(str::to_owned))
        .filter(|link| link.starts_with("/dev/pts/"));
    assert_eq!(launch::tty_of_pid(std::process::id() as i32), expected);
}

#[test]
fn apple_scripts_escape_what_they_carry() {
    let script = launch::terminal_app_script("hgs a claude/a\"b");
    assert!(script.contains("do script"));
    assert!(script.contains("claude/a\\\"b"));
    assert!(launch::terminal_app_script("true").contains("activate"));
    let notice = launch::notification_script("hgs-tray: \"кавычка\"", "строка\nвторая \\ слэш");
    assert!(notice.starts_with("display notification "));
    assert!(notice.contains("with title"));
    assert!(notice.contains("hgs-tray: \\\"кавычка\\\""));
    assert!(!notice.contains('\n'));
    assert!(notice.contains("строка\\nвторая"));
    assert!(notice.contains("\\\\ слэш"));
}

#[test]
fn windows_terminal_starts_the_program_directly() {
    let arguments = strings(&["@mac", "a", "codex/проект/my name"]);
    let title = launch::windows_title(&arguments);
    assert_eq!(title, "Zerus: @mac a codex/проект/my name");
    assert_eq!(
        launch::terminal_arguments(
            WindowsMode::Auto,
            &title,
            "C:/Program Files/Zerus/hgs.exe",
            &arguments
        ),
        Some(strings(&[
            "-w",
            "0",
            "new-tab",
            "--title",
            &title,
            "--",
            "C:\\Program Files\\Zerus\\hgs.exe",
            "@mac",
            "a",
            "codex/проект/my name"
        ]))
    );
    let window = launch::terminal_arguments(
        WindowsMode::Window,
        &title,
        "C:\\Zerus\\hgs.exe",
        &arguments,
    )
    .unwrap();
    assert_eq!(window[..3], strings(&["-w", "new", "new-tab"]));
    let escaped = launch::terminal_arguments(
        WindowsMode::Tab,
        "Zerus: a;b",
        "C:\\hgs.exe",
        &strings(&["a", "x;y"]),
    )
    .unwrap();
    assert_eq!(escaped[4], "Zerus: a\\;b");
    assert_eq!(escaped.last().unwrap(), "x\\;y");
    let long = launch::windows_title(&["n".repeat(200)]);
    assert_eq!(long.encode_utf16().count(), 100);
    assert!(long.ends_with('\u{2026}'));
}

#[test]
fn windows_terminal_refuses_arguments_it_would_change() {
    for argument in [
        "say \"hi\"",
        "",
        "tab\there",
        "trailing \\",
        "line\nbreak",
        "a\\;b",
        "fix %OS% check",
        "deploy %USERNAME%",
        "50%",
    ] {
        let arguments = strings(&["a", argument]);
        assert_eq!(
            launch::terminal_arguments(WindowsMode::Auto, "t", "C:\\hgs.exe", &arguments),
            None,
            "{argument:?}"
        );
    }
    let pair = strings(&["%OS", "x%"]);
    assert_eq!(
        launch::terminal_arguments(WindowsMode::Auto, "t", "C:\\hgs.exe", &pair),
        None
    );
    let program = "C:\\Users\\50%off\\hgs.exe";
    assert_eq!(
        launch::terminal_arguments(WindowsMode::Auto, "t", program, &strings(&["a"])),
        None
    );
    let trailing = strings(&["C:\\dir\\"]);
    assert!(launch::terminal_arguments(WindowsMode::Auto, "t", "C:\\hgs.exe", &trailing).is_some());
}

#[test]
fn windows_terminal_opens_power_shell_in_the_folder() {
    let pwsh = strings(&["C:/Program Files/PowerShell/7/pwsh.exe"]);
    assert_eq!(launch::windows_shell(Some(&pwsh[0]), None), pwsh);
    let windows = launch::windows_shell(None, Some("C:\\Windows"));
    assert_eq!(
        windows,
        strings(&[
            "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe",
            "-NoLogo"
        ])
    );
    assert_eq!(
        launch::shell_arguments(WindowsMode::Auto, "C:/work/my project", &pwsh),
        Some(strings(&[
            "-w",
            "0",
            "new-tab",
            "-d",
            "C:\\work\\my project",
            "--title",
            "Zerus: C:\\work\\my project",
            "--",
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe"
        ]))
    );
    assert_eq!(
        launch::shell_arguments(WindowsMode::Window, "//server/share/dir", &windows),
        Some(strings(&[
            "-w",
            "new",
            "new-tab",
            "-d",
            "\\\\server\\share\\dir",
            "--title",
            "Zerus: \\\\server\\share\\dir",
            "--",
            "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            "-NoLogo"
        ]))
    );
    assert_eq!(
        launch::shell_arguments(WindowsMode::Tab, "c:\\", &pwsh).unwrap()[3..5],
        strings(&["-d", "c:\\"])
    );
    assert_eq!(
        launch::shell_arguments(WindowsMode::Auto, "C:/work/a;b", &pwsh).unwrap()[3..7],
        strings(&["-d", "C:\\work\\a\\;b", "--title", "Zerus: C:\\work\\a\\;b"])
    );
    for folder in ["C:/Users/user/50%off", "C:/work/%OS%", "C:/work/a\\;b", ""] {
        assert_eq!(
            launch::shell_arguments(WindowsMode::Auto, folder, &pwsh),
            None,
            "{folder:?}"
        );
    }
    let odd = strings(&["C:/Users/user/50%off/pwsh.exe"]);
    assert_eq!(
        launch::shell_arguments(WindowsMode::Auto, "C:/work", &odd),
        None
    );
    assert_eq!(
        launch::shell_arguments(WindowsMode::Auto, "C:/work", &[]),
        None
    );
}

#[test]
fn console_host_arguments_carry_the_exact_command() {
    let arguments = strings(&[
        "@mac",
        "a",
        "claude/p/fix \"login\" it's",
        "",
        "C:\\dir\\",
        "50%",
    ]);
    let host = console_host::arguments(
        "Zerus: it's",
        "C:\\Program Files\\Zerus\\hgs.exe",
        &arguments,
    );
    assert_eq!(
        host[..5],
        strings(&[
            "--console-run",
            "--title",
            "Zerus: it's",
            "--",
            "C:\\Program Files\\Zerus\\hgs.exe"
        ])
    );
    let request = console_host::parse(&host).unwrap();
    assert_eq!(request.title, "Zerus: it's");
    assert_eq!(request.program, "C:\\Program Files\\Zerus\\hgs.exe");
    assert_eq!(request.arguments, arguments);
    assert!(
        console_host::parse(&console_host::arguments("t", "hgs", &[]))
            .unwrap()
            .arguments
            .is_empty()
    );
    for malformed in [
        strings(&["--console-run"]),
        strings(&["--console-run", "--title", "t", "--"]),
        strings(&["--console-run", "--title", "t", "hgs", "ls"]),
        strings(&["--console-run", "--title", "t", "--", ""]),
        strings(&["--sessions", "--title", "t", "--", "hgs"]),
    ] {
        assert_eq!(console_host::parse(&malformed), None, "{malformed:?}");
    }
    assert_eq!(
        console_host::failure_message("C:\\Zerus\\hgs.exe", 3),
        "hgs exited with code 3. Press Enter to close this window."
    );
    assert_eq!(
        console_host::failure_message("C:/Zerus/hgs.EXE", 0xC000_013A),
        "hgs exited with code 0xC000013A. Press Enter to close this window."
    );
}

#[test]
fn the_program_line_is_the_qprocess_command_line() {
    assert_eq!(
        shell::windows_program_line(
            "C:/Program Files/Zerus/hgs.exe",
            &strings(&["@mac", "a b", ""])
        ),
        "\"C:\\Program Files\\Zerus\\hgs.exe\"  @mac \"a b\" \"\""
    );
    assert_eq!(shell::windows_program_line("hgs", &[]), "hgs ");
    assert_eq!(shell::windows_program_line("", &strings(&["x"])), " x");
}

#[test]
fn folder_shells_quote_paths_and_use_machine_profiles() {
    use Shell::{Posix, PowerShell};
    assert_eq!(shell::folder_shell("", "", "relative", Posix), None);
    assert_eq!(
        shell::folder_shell("", "", "/work/a folder", Posix).unwrap(),
        "cd -- '/work/a folder' && exec \"${SHELL:-/bin/sh}\" -l"
    );
    assert_eq!(
        shell::folder_shell("/opt/hgs", "mac", "/work/it's $(literal)", Posix).unwrap(),
        shell::command(
            "/opt/hgs",
            &strings(&[
                "machine",
                "ssh",
                "mac",
                "--directory",
                "/work/it's $(literal)"
            ]),
            Posix
        )
    );
    assert_eq!(shell::folder_shell("", "", "C:/work", Posix), None);
    assert_eq!(
        shell::folder_shell("/opt/hgs", "mac", "C:/work", Posix),
        None
    );
    assert_eq!(
        shell::folder_shell("", "", "/work/a folder", PowerShell),
        None
    );
    assert_eq!(shell::folder_shell("", "", "work", PowerShell), None);
    assert_eq!(
        shell::folder_shell("", "", "C:/work/project", PowerShell).unwrap(),
        "Set-Location -LiteralPath C:\\work\\project"
    );
    assert_eq!(
        shell::folder_shell("", "", "c:\\Users\\user\\it's [draft] $x", PowerShell).unwrap(),
        "Set-Location -LiteralPath 'c:\\Users\\user\\it''s [draft] $x'"
    );
    assert_eq!(
        shell::folder_shell("", "", "//server/share/dir", PowerShell).unwrap(),
        "Set-Location -LiteralPath \\\\server\\share\\dir"
    );
    assert_eq!(
        shell::folder_shell("C:/Zerus/hgs.exe", "mac", "C:/work", PowerShell),
        None
    );
    assert_eq!(
        shell::folder_shell("C:/Zerus/hgs.exe", "mac", "/work/a folder", PowerShell).unwrap(),
        "C:\\Zerus\\hgs.exe machine ssh mac --directory '/work/a folder'"
    );
    assert_eq!(shell::folder_shell("", "", "/work/a\0b", Posix), None);
}

#[test]
fn open_modes_follow_the_platform() {
    assert_eq!(launch::open_modes(Platform::Mac), ["terminal", "clipboard"]);
    assert_eq!(
        launch::open_modes(Platform::Windows),
        ["auto", "tab", "window", "console", "clipboard"]
    );
    assert_eq!(
        launch::open_modes(Platform::Linux),
        ["auto", "tab", "window", "clipboard"]
    );
    assert_eq!(launch::default_open_mode(Platform::Mac), "terminal");
    let settings = |mode: &str| Settings {
        template: String::new(),
        open_mode: mode.to_owned(),
    };
    // A mode of another platform, or none, is the default here.
    assert_eq!(settings("console").mode(Platform::Linux), "auto");
    assert_eq!(settings("").mode(Platform::Windows), "auto");
    assert_eq!(settings("tab").mode(Platform::Mac), "terminal");
    assert_eq!(settings("window").mode(Platform::Linux), "window");
    for (mode, expected) in [
        ("auto", WindowsMode::Auto),
        ("tab", WindowsMode::Tab),
        ("window", WindowsMode::Window),
        ("console", WindowsMode::Console),
        ("clipboard", WindowsMode::Auto),
    ] {
        assert_eq!(WindowsMode::from_mode(mode), expected);
    }
}

#[test]
fn launches_pick_the_backend_qt_picks() {
    let settings = |template: &str, mode: &str| Settings {
        template: template.to_owned(),
        open_mode: mode.to_owned(),
    };
    let everything = |_: &str| true;
    let nothing = |_: &str| false;
    let xdg_only = |program: &str| program == "xdg-terminal-exec";
    let foot = settings("foot -e bash -lc {cmd}", "window");
    for platform in [
        Platform::Linux,
        Platform::Mac,
        Platform::Windows,
        Platform::Other,
    ] {
        assert_eq!(
            launch::launch_backend(&foot, platform, &everything),
            Backend::Command {
                template: foot.template.clone()
            }
        );
    }
    assert_eq!(
        launch::launch_backend(&settings("", "tab"), Platform::Linux, &everything),
        Backend::Konsole {
            mode: KonsoleMode::Tab
        }
    );
    // A launch always opens: clipboard is the platform's default here.
    assert_eq!(
        launch::launch_backend(&settings("", "clipboard"), Platform::Linux, &everything),
        Backend::Konsole {
            mode: KonsoleMode::Auto
        }
    );
    assert_eq!(
        launch::launch_backend(&settings("", "tab"), Platform::Linux, &xdg_only),
        Backend::Command {
            template: "xdg-terminal-exec bash -lc {cmd}".to_owned()
        }
    );
    assert_eq!(
        launch::launch_backend(&settings("", "window"), Platform::Linux, &nothing),
        Backend::Konsole {
            mode: KonsoleMode::Window
        }
    );
    assert_eq!(
        launch::launch_backend(&settings("", "clipboard"), Platform::Mac, &nothing),
        Backend::TerminalApp
    );
    assert_eq!(
        launch::launch_backend(&settings("", "console"), Platform::Windows, &nothing),
        Backend::WindowsTerminal {
            mode: WindowsMode::Console
        }
    );
    assert_eq!(
        launch::launch_backend(&settings("", "auto"), Platform::Other, &everything),
        Backend::Command {
            template: String::new()
        }
    );
}

#[derive(Default)]
struct Log {
    starts: Vec<Start>,
    konsole: Vec<String>,
}

type Shared = Arc<Mutex<Log>>;

struct FakeDesktop {
    installed: HashMap<String, String>,
    env: HashMap<String, String>,
    exists: Vec<String>,
    failing: Vec<String>,
    log: Shared,
}

impl FakeDesktop {
    fn new(log: &Shared, installed: &[&str]) -> Self {
        Self {
            installed: installed
                .iter()
                .map(|program| ((*program).to_owned(), format!("/usr/bin/{program}")))
                .collect(),
            env: HashMap::new(),
            exists: Vec::new(),
            failing: Vec::new(),
            log: log.clone(),
        }
    }
}

impl Desktop for FakeDesktop {
    fn find(&self, program: &str) -> Option<String> {
        self.installed.get(program).cloned()
    }
    fn env(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }
    fn exists(&self, path: &str) -> bool {
        self.exists.iter().any(|existing| existing == path)
    }
    fn home(&self) -> String {
        "/home/user".to_owned()
    }
    fn console_host(&self) -> String {
        "C:/Program Files/Zerus/zerus.exe".to_owned()
    }
    fn start(&mut self, start: &Start) -> Result<(), String> {
        if self.failing.contains(&start.program) {
            return Err("The system cannot find the file specified.".to_owned());
        }
        self.log.lock().unwrap().starts.push(start.clone());
        Ok(())
    }
}

/// One Konsole process with windows 1 and 2 and sessions 7 and 8.
struct FakeKonsole {
    services: Vec<String>,
    input: bool,
    active: Option<i32>,
    raises: bool,
    new_session: Result<i32, String>,
    run_command: Result<(), String>,
    log: Shared,
}

impl FakeKonsole {
    fn new(log: &Shared) -> Self {
        Self {
            services: vec!["org.kde.konsole-42".to_owned()],
            input: true,
            active: None,
            raises: true,
            new_session: Ok(9),
            run_command: Ok(()),
            log: log.clone(),
        }
    }

    fn note(&self, call: String) {
        self.log.lock().unwrap().konsole.push(call);
    }
}

impl Konsole for FakeKonsole {
    fn services(&mut self) -> Vec<String> {
        self.services.clone()
    }
    fn children(&mut self, _: &str, path: &str) -> Vec<i32> {
        match path {
            "/Windows" => vec![1, 2],
            "/Sessions" => vec![7, 8],
            _ => Vec::new(),
        }
    }
    fn window_flag(&mut self, _: &str, window: i32, property: &str) -> Option<bool> {
        match property {
            "isActiveWindow" => Some(self.active == Some(window)),
            "minimized" => Some(window == 2),
            _ => None,
        }
    }
    fn new_session(&mut self, service: &str, window: i32) -> Result<i32, String> {
        self.note(format!("newSession {service} {window}"));
        self.new_session.clone()
    }
    fn send_text(&mut self, _: &str, session: i32, text: &str) -> Result<(), String> {
        self.note(format!("sendText {session} {text:?}"));
        if self.input {
            Ok(())
        } else {
            Err(
                "org.freedesktop.DBus.Error.AccessDenied: Security sensitive DBus API is disabled"
                    .to_owned(),
            )
        }
    }
    fn run_command(&mut self, _: &str, session: i32, command: &str) -> Result<(), String> {
        self.note(format!("runCommand {session} {command}"));
        self.run_command.clone()
    }
    fn set_current_session(&mut self, _: &str, window: i32, session: i32) {
        self.note(format!("setCurrentSession {window} {session}"));
    }
    fn window_call(&mut self, _: &str, window: i32, method: &str) {
        self.note(format!("{method} {window}"));
        if method == "raise" && self.raises {
            self.active = Some(window);
        }
    }
    fn process_id(&mut self, _: &str, session: i32) -> Option<i32> {
        Some(1000 + session)
    }
    fn session_list(&mut self, _: &str, window: i32) -> Option<Vec<String>> {
        Some(if window == 2 {
            strings(&["8"])
        } else {
            strings(&["7"])
        })
    }
    fn tty_of(&mut self, pid: i32) -> Option<String> {
        Some(format!("/dev/pts/{}", pid - 1000))
    }
    fn settle(&mut self) {}
}

fn launcher(
    platform: Platform,
    mode: &str,
    desktop: FakeDesktop,
    konsole: FakeKonsole,
) -> Launcher {
    let settings = Settings {
        template: String::new(),
        open_mode: mode.to_owned(),
    };
    Launcher::with(settings, platform, Box::new(desktop), Box::new(konsole))
}

fn starts(log: &Shared) -> Vec<Start> {
    std::mem::take(&mut log.lock().unwrap().starts)
}

fn konsole_calls(log: &Shared) -> Vec<String> {
    std::mem::take(&mut log.lock().unwrap().konsole)
}

fn start(program: &str, arguments: Vec<String>, directory: &str, window: Window) -> Start {
    Start {
        program: program.to_owned(),
        arguments,
        directory: directory.to_owned(),
        window,
    }
}

fn session() -> Vec<String> {
    strings(&["@mac", "a", "codex/p/a b"])
}

#[test]
fn a_template_runs_the_displayed_command() {
    let log = Shared::default();
    let settings = Settings {
        template: "foot -e bash -lc {cmd}".to_owned(),
        open_mode: "clipboard".to_owned(),
    };
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let konsole = FakeKonsole::new(&log);
    let mut launcher = Launcher::with(
        settings,
        Platform::Linux,
        Box::new(desktop),
        Box::new(konsole),
    );
    assert!(launcher.copies());
    assert_eq!(launcher.open_program("/opt/hgs", &session()), Ok(None));
    let command = shell::command("/opt/hgs", &session(), Shell::Posix);
    assert_eq!(
        starts(&log),
        [start(
            "foot",
            strings(&["-e", "bash", "-lc", &command]),
            "/home/user",
            Window::Plain
        )]
    );
    assert_eq!(launcher.open_shell("/work/a folder"), Ok(None));
    let shell = shell::folder_shell("", "", "/work/a folder", Shell::Posix).unwrap();
    assert_eq!(starts(&log)[0].arguments[3], shell);
    assert_eq!(
        launcher.open_shell("work"),
        Err("not a folder of this computer: work".to_owned())
    );
    assert!(starts(&log).is_empty());
    assert!(konsole_calls(&log).is_empty());
}

#[test]
fn a_template_that_cannot_start_names_its_program() {
    let log = Shared::default();
    let mut desktop = FakeDesktop::new(&log, &[]);
    desktop.failing.push("foot".to_owned());
    let settings = Settings {
        template: "foot -e bash -lc {cmd}".to_owned(),
        open_mode: String::new(),
    };
    let mut launcher = Launcher::with(
        settings,
        Platform::Linux,
        Box::new(desktop),
        Box::new(FakeKonsole::new(&log)),
    );
    assert_eq!(
        launcher.open_program("hgs", &session()),
        Err("cannot start 'foot'".to_owned())
    );
}

#[test]
fn a_windows_template_gets_a_console_and_a_power_shell_command() {
    let log = Shared::default();
    let settings = Settings {
        template: "powershell.exe -NoExit -Command {cmd}".to_owned(),
        open_mode: "console".to_owned(),
    };
    let desktop = FakeDesktop::new(&log, &["wt"]);
    let mut launcher = Launcher::with(
        settings,
        Platform::Windows,
        Box::new(desktop),
        Box::new(FakeKonsole::new(&log)),
    );
    assert_eq!(
        launcher.open_program("C:/Zerus/hgs.exe", &session()),
        Ok(None)
    );
    let command = shell::command("C:/Zerus/hgs.exe", &session(), Shell::PowerShell);
    assert_eq!(
        starts(&log),
        [start(
            "powershell.exe",
            strings(&["-NoExit", "-Command", &command]),
            "/home/user",
            Window::Console
        )]
    );
}

#[test]
fn konsole_window_mode_never_touches_the_bus() {
    let log = Shared::default();
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let mut launcher = launcher(Platform::Linux, "window", desktop, FakeKonsole::new(&log));
    assert_eq!(launcher.open_program("hgs", &session()), Ok(None));
    let command = shell::command("hgs", &session(), Shell::Posix);
    assert_eq!(
        starts(&log),
        [start(
            "konsole",
            launch::konsole_launch_args(&command),
            "/home/user",
            Window::Plain
        )]
    );
    assert!(konsole_calls(&log).is_empty());
}

#[test]
fn konsole_tabs_go_to_the_active_window() {
    let log = Shared::default();
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let mut konsole = FakeKonsole::new(&log);
    konsole.active = Some(2);
    let mut launcher = launcher(Platform::Linux, "auto", desktop, konsole);
    assert_eq!(launcher.open_program("hgs", &session()), Ok(None));
    let command = shell::command("hgs", &session(), Shell::Posix);
    assert_eq!(
        konsole_calls(&log),
        strings(&[
            "sendText 7 \"\"",
            "newSession org.kde.konsole-42 2",
            &format!("runCommand 9 {command}"),
            "setCurrentSession 2 9",
            "showNormal 2",
            "raise 2"
        ])
    );
    assert!(starts(&log).is_empty());
}

#[test]
fn a_refused_tab_mode_explains_the_window_once() {
    let log = Shared::default();
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let mut konsole = FakeKonsole::new(&log);
    konsole.input = false;
    let mut launcher = launcher(Platform::Linux, "tab", desktop, konsole);
    let advisory = launcher.open_program("hgs", &session()).unwrap().unwrap();
    assert!(advisory.contains("EnableSecuritySensitiveDBusAPI=true"));
    assert!(advisory.contains("/home/user/.config/konsolerc"));
    assert_eq!(starts(&log).len(), 1);
    // No empty tab is created when Konsole refuses input.
    assert!(!konsole_calls(&log)
        .iter()
        .any(|call| call.starts_with("newSession")));
    assert_eq!(launcher.open_program("hgs", &session()), Ok(None));
    assert_eq!(starts(&log).len(), 1);
}

#[test]
fn auto_mode_falls_back_to_a_window_quietly() {
    let log = Shared::default();
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let mut konsole = FakeKonsole::new(&log);
    konsole.input = false;
    let mut launcher = launcher(Platform::Linux, "auto", desktop, konsole);
    assert_eq!(launcher.open_program("hgs", &session()), Ok(None));
    assert_eq!(starts(&log)[0].program, "konsole");
}

#[test]
fn a_closed_window_gets_a_new_one_and_a_refused_command_fails() {
    let log = Shared::default();
    let mut konsole = FakeKonsole::new(&log);
    konsole.new_session = Err("no reply".to_owned());
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let mut closed = launcher(Platform::Linux, "tab", desktop, konsole);
    assert_eq!(closed.open_program("hgs", &session()), Ok(None));
    assert_eq!(starts(&log)[0].program, "konsole");

    let mut konsole = FakeKonsole::new(&log);
    konsole.run_command = Err("org.freedesktop.DBus.Error.NoReply: timeout".to_owned());
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let mut refused = launcher(Platform::Linux, "tab", desktop, konsole);
    assert_eq!(
        refused.open_program("hgs", &session()),
        Err(
            "Konsole rejected the command (org.freedesktop.DBus.Error.NoReply: timeout)".to_owned()
        )
    );
    assert!(starts(&log).is_empty());
}

#[test]
fn an_attached_session_comes_forward_instead_of_a_second_client() {
    let log = Shared::default();
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let mut launcher = launcher(Platform::Linux, "auto", desktop, FakeKonsole::new(&log));
    assert_eq!(
        launcher.open_session("hgs", "mac", "codex/p", "pts/8"),
        Ok(Opened::Raised)
    );
    assert_eq!(
        konsole_calls(&log),
        strings(&["setCurrentSession 2 8", "showNormal 2", "raise 2"])
    );
    assert!(starts(&log).is_empty());
}

#[test]
fn a_window_the_compositor_keeps_back_opens_a_tab() {
    let log = Shared::default();
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let mut konsole = FakeKonsole::new(&log);
    konsole.raises = false;
    let mut launcher = launcher(Platform::Linux, "auto", desktop, konsole);
    assert_eq!(
        launcher.open_session("/opt/hgs", "", "codex/p", "/dev/pts/7"),
        Ok(Opened::Opened { advisory: None })
    );
    let calls = konsole_calls(&log);
    assert!(
        calls.contains(&"runCommand 9 /opt/hgs a codex/p".to_owned()),
        "{calls:?}"
    );
    // An unknown tty skips straight to opening.
    assert_eq!(
        launcher.open_session("", "", "codex/p", "/dev/pts/3"),
        Ok(Opened::Opened { advisory: None })
    );
    assert!(konsole_calls(&log).contains(&"runCommand 9 hgs a codex/p".to_owned()));
}

#[test]
fn terminal_app_opens_through_osascript() {
    let log = Shared::default();
    let desktop = FakeDesktop::new(&log, &[]);
    let mut launcher = launcher(Platform::Mac, "clipboard", desktop, FakeKonsole::new(&log));
    assert!(launcher.copies());
    assert_eq!(
        launcher.open_session("/opt/hgs", "mac", "codex/p", "/dev/pts/8"),
        Ok(Opened::Opened { advisory: None })
    );
    let command = shell::open_command("/opt/hgs", "mac", "codex/p", None, Shell::Posix);
    assert_eq!(
        starts(&log),
        [start(
            "osascript",
            strings(&["-e", &launch::terminal_app_script(&command)]),
            "/home/user",
            Window::Plain
        )]
    );
    assert!(konsole_calls(&log).is_empty());
}

#[test]
fn windows_terminal_runs_the_program_or_reports_the_console() {
    let log = Shared::default();
    let hgs = "C:/Zerus/hgs.exe";
    let title = launch::windows_title(&session());
    let desktop = FakeDesktop::new(&log, &["wt"]);
    let mut tab = launcher(Platform::Windows, "tab", desktop, FakeKonsole::new(&log));
    assert_eq!(tab.open_program(hgs, &session()), Ok(None));
    let wt = launch::terminal_arguments(WindowsMode::Tab, &title, hgs, &session()).unwrap();
    assert_eq!(
        starts(&log),
        [start("/usr/bin/wt", wt, "/home/user", Window::Plain)]
    );

    // Arguments Windows Terminal would change go to the console host, and an
    // explicit tab says so.
    let awkward = strings(&["a", "say \"hi\""]);
    let advisory = tab.open_program(hgs, &awkward).unwrap().unwrap();
    assert_eq!(
        advisory,
        "Windows Terminal cannot pass these arguments unchanged, so the session opened in a \
         console window. Choose Console in \"Open in\" to use console windows deliberately."
    );
    let host = console_host::arguments(&launch::windows_title(&awkward), hgs, &awkward);
    assert_eq!(
        starts(&log),
        [start(
            "C:/Program Files/Zerus/zerus.exe",
            host,
            "/home/user",
            Window::Shown
        )]
    );

    let desktop = FakeDesktop::new(&log, &["wt"]);
    let mut auto = launcher(Platform::Windows, "auto", desktop, FakeKonsole::new(&log));
    assert_eq!(auto.open_program(hgs, &awkward), Ok(None));
    assert_eq!(starts(&log)[0].window, Window::Shown);

    let desktop = FakeDesktop::new(&log, &["wt"]);
    let mut console = launcher(
        Platform::Windows,
        "console",
        desktop,
        FakeKonsole::new(&log),
    );
    assert_eq!(console.open_program(hgs, &session()), Ok(None));
    assert_eq!(starts(&log)[0].program, "C:/Program Files/Zerus/zerus.exe");
}

#[test]
fn windows_terminal_is_found_by_its_alias_or_reported_missing() {
    let log = Shared::default();
    let hgs = "C:/Zerus/hgs.exe";
    let mut desktop = FakeDesktop::new(&log, &[]);
    desktop.env.insert(
        "LOCALAPPDATA".to_owned(),
        "C:\\Users\\user\\AppData\\Local".to_owned(),
    );
    let alias = "C:/Users/user/AppData/Local/Microsoft/WindowsApps/wt.exe";
    desktop.exists.push(alias.to_owned());
    desktop.failing.push(alias.to_owned());
    let mut window = launcher(Platform::Windows, "window", desktop, FakeKonsole::new(&log));
    let advisory = window.open_program(hgs, &session()).unwrap().unwrap();
    assert!(advisory.starts_with(
        "Windows Terminal (C:\\Users\\user\\AppData\\Local\\Microsoft\\WindowsApps\\wt.exe) did not start, so the session"
    ));

    let mut desktop = FakeDesktop::new(&log, &[]);
    desktop
        .failing
        .push("C:/Program Files/Zerus/zerus.exe".to_owned());
    let mut missing = launcher(Platform::Windows, "tab", desktop, FakeKonsole::new(&log));
    assert_eq!(
        missing.open_program(hgs, &session()),
        Err("cannot start C:\\Program Files\\Zerus\\zerus.exe: The system cannot find the file specified."
            .to_owned())
    );
    let desktop = FakeDesktop::new(&log, &[]);
    let mut missing = launcher(Platform::Windows, "tab", desktop, FakeKonsole::new(&log));
    assert!(missing
        .open_program(hgs, &session())
        .unwrap()
        .unwrap()
        .starts_with("Windows Terminal (wt.exe) is not installed, so the session opened"));
}

#[test]
fn windows_folder_shells_start_in_the_folder() {
    let log = Shared::default();
    let mut desktop = FakeDesktop::new(&log, &["wt"]);
    desktop.installed.insert(
        "pwsh".to_owned(),
        "C:/Program Files/PowerShell/7/pwsh.exe".to_owned(),
    );
    let mut launcher = launcher(Platform::Windows, "auto", desktop, FakeKonsole::new(&log));
    assert_eq!(launcher.open_shell("C:/work/my project"), Ok(None));
    let pwsh = strings(&["C:/Program Files/PowerShell/7/pwsh.exe"]);
    let wt = launch::shell_arguments(WindowsMode::Auto, "C:\\work\\my project", &pwsh).unwrap();
    assert_eq!(
        starts(&log),
        [start("/usr/bin/wt", wt, "/home/user", Window::Plain)]
    );

    // A folder Windows Terminal would change opens in a console there.
    assert_eq!(launcher.open_shell("C:/work/50%off"), Ok(None));
    let host = console_host::arguments("Zerus: C:\\work\\50%off", &pwsh[0], &[]);
    assert_eq!(
        starts(&log),
        [start(
            "C:/Program Files/Zerus/zerus.exe",
            host,
            "C:\\work\\50%off",
            Window::Shown
        )]
    );
    assert_eq!(
        launcher.open_shell("/work"),
        Err("not a folder of this computer: /work".to_owned())
    );
}

#[test]
fn folder_shells_of_peers_go_through_machine_ssh() {
    let log = Shared::default();
    let desktop = FakeDesktop::new(&log, &["konsole"]);
    let mut launcher = launcher(Platform::Linux, "window", desktop, FakeKonsole::new(&log));
    assert_eq!(
        launcher.open_folder_shell("/opt/hgs", "mac", "/work/a b"),
        Ok(None)
    );
    let arguments = strings(&["machine", "ssh", "mac", "--directory", "/work/a b"]);
    let command = shell::command("/opt/hgs", &arguments, Shell::Posix);
    assert_eq!(
        starts(&log)[0].arguments,
        launch::konsole_launch_args(&command)
    );
    assert_eq!(
        launcher.open_folder_shell("/opt/hgs", "", "/work/a b"),
        Ok(None)
    );
    let local = shell::folder_shell("", "", "/work/a b", Shell::Posix).unwrap();
    assert_eq!(
        starts(&log)[0].arguments,
        launch::konsole_launch_args(&local)
    );
    assert_eq!(
        launcher.open_folder_shell("/opt/hgs", "mac", "C:/work"),
        Err("not a folder of mac: C:/work".to_owned())
    );
    assert!(starts(&log).is_empty());
}

#[test]
fn launch_arguments_name_the_computer_first() {
    assert_eq!(
        launch::sign_in_arguments("mac", "acct-1"),
        strings(&["@mac", "account", "login", "acct-1"])
    );
    assert_eq!(
        launch::install_arguments("", "claude"),
        strings(&["account", "install", "claude"])
    );
    assert_eq!(
        launch::ssh_arguments("mac"),
        strings(&["machine", "ssh", "mac"])
    );
    assert_eq!(
        launch::not_opened("cannot start osascript", "hgs a x"),
        "cannot start osascript\n\nCommand: hgs a x\nRun it in a terminal by hand."
    );
}

#[cfg(unix)]
#[test]
fn the_system_finds_programs_on_path() {
    let system = launch::System;
    assert!(system.find("sh").is_some());
    assert_eq!(system.find("zerus-no-such-program"), None);
}

/// A started program runs in a session of its own and inherits no descriptor
/// beyond standard input, output and error.
#[cfg(target_os = "linux")]
#[test]
fn the_system_starts_detached_without_leaking_descriptors() {
    use std::os::fd::AsRawFd;
    let dir = std::env::temp_dir().join(format!("zerus-terminal-launch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A descriptor without close-on-exec, as a careless library leaves one.
    let leaked = std::fs::File::open("/dev/null").unwrap();
    let fd = leaked.as_raw_fd();
    // SAFETY: fcntl on a descriptor this test owns.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
    }
    let out = dir.join("out");
    let script = format!(
        "set -- $(cat /proc/$$/stat | sed 's/.*) //'); \
         if [ -e /proc/$$/fd/{fd} ]; then leak=yes; else leak=no; fi; \
         echo \"$$ $4 $leak $(pwd)\" > {}.tmp && mv {}.tmp {}",
        out.display(),
        out.display(),
        out.display()
    );
    let mut system = launch::System;
    system
        .start(&start(
            "sh",
            strings(&["-c", &script]),
            dir.to_str().unwrap(),
            Window::Plain,
        ))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let line = loop {
        if let Ok(line) = std::fs::read_to_string(&out) {
            break line;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the program did not run"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let fields: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(fields[0], fields[1], "its own session: {line}");
    assert_eq!(fields[2], "no", "descriptor {fd} leaked: {line}");
    assert_eq!(fields[3], dir.to_str().unwrap());
    let error = system
        .start(&start(
            "/nonexistent/zerus-terminal",
            Vec::new(),
            "/",
            Window::Plain,
        ))
        .unwrap_err();
    assert!(error.contains("No such file"), "{error}");
    drop(leaked);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Konsole itself over a private session bus: `tests/support/konsole_bus.sh`
/// starts an offscreen Konsole with its own home and bus, then runs this with
/// `ZERUS_TEST_KONSOLE` set to `on` (tabs allowed over D-Bus) or `off`.
#[cfg(all(target_os = "linux", feature = "konsole"))]
#[test]
#[ignore = "needs an isolated Konsole: tests/support/konsole_bus.sh"]
fn konsole_on_a_private_bus() {
    let tabs = match std::env::var("ZERUS_TEST_KONSOLE").as_deref() {
        Ok("on") => true,
        Ok("off") => false,
        _ => panic!("run through tests/support/konsole_bus.sh"),
    };
    let out = std::path::PathBuf::from(std::env::var("ZERUS_TEST_KONSOLE_OUT").unwrap());
    let mut bus = launch::DbusKonsole::new();
    let services = bus.services();
    assert_eq!(services.len(), 1, "{services:?}");
    let sessions = bus.children(&services[0], "/Sessions");
    assert!(!sessions.is_empty());
    assert_eq!(bus.send_text(&services[0], sessions[0], "").is_ok(), tabs);

    let settings = Settings {
        template: String::new(),
        open_mode: "tab".to_owned(),
    };
    let mut launcher = Launcher::with(
        settings,
        Platform::Linux,
        Box::new(launch::System),
        Box::new(launch::DbusKonsole::new()),
    );
    let script = format!("echo opened > {0}.tmp && mv {0}.tmp {0}", out.display());
    let advisory = launcher
        .open_program("sh", &strings(&["-c", &script]))
        .unwrap();
    assert_eq!(advisory.is_some(), !tabs, "{advisory:?}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !out.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the command did not run"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let after = bus.children(&services[0], "/Sessions");
    // A tab adds a session to this Konsole; a window is another process,
    // which closes once its command ends.
    assert_eq!(
        after.len() > sessions.len(),
        tabs,
        "{sessions:?} -> {after:?}"
    );
    if tabs {
        let tab = *after.last().unwrap();
        let pid = bus.process_id(&services[0], tab).unwrap();
        let tty = launch::tty_of_pid(pid).unwrap();
        // `true` stands in for hgs: nothing may reach real sessions.
        let opened = launcher.open_session("true", "", "codex/p", &tty).unwrap();
        println!("raise {tty}: {opened:?}");
    }
    println!(
        "konsole tabs={tabs} sessions {sessions:?} -> {after:?}, advisory: {}",
        advisory.is_some()
    );
}
