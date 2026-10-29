//! Terminals outside the client: a port of the Qt GUI's terminal backends
//! (`tray/src/TerminalBackend.h` with `CommandBackend`, `KonsoleBackend`,
//! `MacBackend`, `WindowsTerminalBackend` and `ConsoleHost`) and of the
//! `TrayAgent` actions that use them: `makeLaunchTerminal`,
//! `openSessionTerminal`, the folder shell, agent sign-in and install, and the
//! SSH terminal.
//!
//! The rules are pure and tested on every platform. A [`Desktop`] finds and
//! starts programs and a [`Konsole`] talks to Konsole over D-Bus; both are
//! injected, and [`Launcher::new`] uses the real ones. Every call blocks (a
//! Konsole D-Bus call may take 3 s), so callers run it on a worker thread.
//!
//! A launch always opens a terminal: the `clipboard` open mode copies only a
//! session click ([`Launcher::copies`]), and launches open the platform's
//! default mode instead, as `makeLaunchTerminal` does.
//!
//! As in Qt, Konsole explains a refused tab once per run for session clicks
//! and New session launches, which share the terminal the Qt GUI keeps, and
//! every time for the other launches, whose terminal it builds anew.

pub mod console_host;
#[cfg(all(target_os = "linux", feature = "konsole"))]
mod konsole_bus;
mod system;

use crate::paths::{self, Style};
use crate::shell::{self, Shell};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(all(target_os = "linux", feature = "konsole"))]
pub use konsole_bus::DbusKonsole;
pub use system::System;

/// Title of the notice for [`Launcher`]'s advisory: the terminal opened, but
/// not as asked (`TerminalBackend::advisoryTitle`).
pub const ADVISORY_TITLE: &str = "Zerus: session opened, but not the way you asked";
/// Title of the notice when a session's terminal did not open.
pub const NOT_OPENED_TITLE: &str = "Zerus: the terminal did not open";
/// Notice titles of the other launch actions, as the Qt GUI shows them.
pub const FOLDER_SHELL_FAILED: &str = "Could not open folder shell";
pub const SIGN_IN_FAILED: &str = "Could not open agent sign-in";
pub const INSTALL_FAILED: &str = "Could not open agent installer";
pub const SSH_FAILED: &str = "Could not open SSH terminal";

/// The body of the [`NOT_OPENED_TITLE`] notice: the error and the command to
/// run by hand.
pub fn not_opened(error: &str, command: &str) -> String {
    format!("{error}\n\nCommand: {command}\nRun it in a terminal by hand.")
}

/// The platform whose terminals a launch uses (`AppConfig::Platform`, with
/// other Unix systems apart: they only run a `terminal=` template).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Linux,
    Mac,
    Windows,
    Other,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Other
        }
    }

    /// The shell of a command line a terminal runs here.
    pub fn shell(self) -> Shell {
        if self == Self::Windows {
            Shell::PowerShell
        } else {
            Shell::Posix
        }
    }
}

/// The open modes of `platform`, the default first (`AppConfig::openModes`).
pub fn open_modes(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Mac => &["terminal", "clipboard"],
        Platform::Windows => &["auto", "tab", "window", "console", "clipboard"],
        Platform::Linux | Platform::Other => &["auto", "tab", "window", "clipboard"],
    }
}

pub fn default_open_mode(platform: Platform) -> &'static str {
    open_modes(platform)[0]
}

/// The terminal settings: `terminal=` and `open-mode` of the Qt GUI.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// A command template with `{cmd}`; empty for the platform's terminal.
    #[serde(default)]
    pub template: String,
    /// One of [`open_modes`]; empty or unknown means the default.
    #[serde(default)]
    pub open_mode: String,
}

impl Settings {
    /// The mode in effect: the default for an empty or unknown one, as
    /// `AppConfig` reads `open-mode`.
    pub fn mode(&self, platform: Platform) -> &str {
        if open_modes(platform).contains(&self.open_mode.as_str()) {
            &self.open_mode
        } else {
            default_open_mode(platform)
        }
    }
}

/// How Konsole opens a launch: a tab, quietly a window when Konsole refuses
/// tabs (`auto`), or always a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KonsoleMode {
    Auto,
    Tab,
    Window,
}

/// How Windows opens a launch: a Windows Terminal tab or window, or a console
/// window (also the fallback, reported unless the mode is `auto`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsMode {
    Auto,
    Tab,
    Window,
    Console,
}

impl WindowsMode {
    /// `WindowsTerminalBackend::modeFromString`.
    pub fn from_mode(mode: &str) -> Self {
        match mode {
            "tab" => Self::Tab,
            "window" => Self::Window,
            "console" => Self::Console,
            _ => Self::Auto,
        }
    }
}

/// The terminal a launch uses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Backend {
    /// A command template: `terminal=`, the desktop's default terminal on
    /// Linux without Konsole, or an empty one elsewhere, which fails with an
    /// explanation.
    Command {
        template: String,
    },
    Konsole {
        mode: KonsoleMode,
    },
    /// Terminal.app through `osascript`, always a new window.
    TerminalApp,
    WindowsTerminal {
        mode: WindowsMode,
    },
}

/// The backend of a launch (`makeLaunchTerminal`): a template wins; Linux
/// without Konsole uses the desktop's default terminal when it has one.
pub fn launch_backend(
    settings: &Settings,
    platform: Platform,
    installed: &dyn Fn(&str) -> bool,
) -> Backend {
    let mode = match settings.mode(platform) {
        "clipboard" => default_open_mode(platform),
        mode => mode,
    };
    if !settings.template.is_empty() {
        return Backend::Command {
            template: settings.template.clone(),
        };
    }
    match platform {
        Platform::Linux => {
            if !installed("konsole") {
                if let Some(template) = system_terminal_template(installed) {
                    return Backend::Command {
                        template: template.to_owned(),
                    };
                }
            }
            let mode = match mode {
                "tab" => KonsoleMode::Tab,
                "window" => KonsoleMode::Window,
                _ => KonsoleMode::Auto,
            };
            Backend::Konsole { mode }
        }
        Platform::Mac => Backend::TerminalApp,
        Platform::Windows => Backend::WindowsTerminal {
            mode: WindowsMode::from_mode(mode),
        },
        Platform::Other => Backend::Command {
            template: String::new(),
        },
    }
}

/// The template of the desktop's default terminal
/// (`CommandBackend::systemTerminalTemplate`): `xdg-terminal-exec`, which
/// honours the user's choice, else Debian's `x-terminal-emulator`. A login
/// shell, because a new window lacks the profile that puts `hgs` on PATH.
pub fn system_terminal_template(installed: &dyn Fn(&str) -> bool) -> Option<&'static str> {
    if installed("xdg-terminal-exec") {
        Some("xdg-terminal-exec bash -lc {cmd}")
    } else if installed("x-terminal-emulator") {
        Some("x-terminal-emulator -e bash -lc {cmd}")
    } else {
        None
    }
}

/// `QProcess::splitCommand`: words separated by white space; double quotes
/// group, and three in a row are one literal quote.
pub fn split_command(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quotes = 0;
    let mut quoted = false;
    for c in command.chars() {
        if c == '"' {
            quotes += 1;
            if quotes == 3 {
                quotes = 0;
                word.push(c);
            }
            continue;
        }
        if quotes != 0 {
            if quotes == 1 {
                quoted = !quoted;
            }
            quotes = 0;
        }
        if !quoted && c.is_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// The program and arguments of a template with `command` in place of every
/// `{cmd}` (`CommandBackend::buildArgv`). The template is split first, so the
/// command stays one argument whatever quotes or spaces it has.
pub fn build_argv(template: &str, command: &str) -> Result<Vec<String>, String> {
    if template.trim().is_empty() {
        return Err(
            "no terminal template: set terminal=<template with {cmd}> in \
                    ~/.config/hgs/tray.conf or pass --terminal"
                .to_owned(),
        );
    }
    let mut argv = split_command(template);
    if argv.is_empty() {
        return Err(format!(
            "terminal template '{template}' does not parse into a command"
        ));
    }
    let mut substituted = false;
    for argument in &mut argv {
        if argument.contains("{cmd}") {
            *argument = argument.replace("{cmd}", command);
            substituted = true;
        }
    }
    if !substituted {
        return Err(format!(
            "terminal template '{template}' has no {{cmd}} -- nowhere to put the command"
        ));
    }
    Ok(argv)
}

/// `konsole` arguments that run `command` in a new window, in a login shell
/// (`KonsoleBackend::launchArgs`).
pub fn konsole_launch_args(command: &str) -> Vec<String> {
    ["-e", "bash", "-lc", command].map(str::to_owned).to_vec()
}

/// The pseudo-terminal of a kernel device number from `/proc/PID/stat`
/// (`KonsoleBackend::ttyFromDevNumber`); None for anything but UNIX98 pts.
pub fn tty_from_dev_number(device: u32) -> Option<String> {
    if device == 0 {
        return None;
    }
    let major = (device >> 8) & 0xfff;
    let minor = (device & 0xff) | ((device >> 12) & 0xfff00);
    (136..=143)
        .contains(&major)
        .then(|| format!("/dev/pts/{}", (major - 136) * 256 + minor))
}

/// The pseudo-terminal of a process (`KonsoleBackend::ttyOfPid`): its
/// controlling terminal, else what its standard input points to.
pub fn tty_of_pid(pid: i32) -> Option<String> {
    if let Ok(line) = std::fs::read(format!("/proc/{pid}/stat")) {
        // The command name in parentheses may hold spaces and ')'.
        let end = line.iter().rposition(|&byte| byte == b')').unwrap_or(0);
        if end > 0 {
            let rest = line.get(end + 2..).unwrap_or_default();
            // After it: state, ppid, pgrp, session, tty_nr.
            let device = rest
                .split(|&byte| byte == b' ')
                .nth(4)
                .and_then(|field| std::str::from_utf8(field).ok()?.parse().ok());
            if let Some(tty) = device.and_then(tty_from_dev_number) {
                return Some(tty);
            }
        }
    }
    let link = std::fs::read_link(format!("/proc/{pid}/fd/0")).ok()?;
    let link = link.to_str()?;
    link.starts_with("/dev/pts/").then(|| link.to_owned())
}

/// `pts/4` and `/dev/pts/4` alike as `/dev/pts/4`; None when blank.
pub fn normalize_tty(tty: &str) -> Option<String> {
    let tty = tty.trim();
    if tty.is_empty() {
        None
    } else if tty.starts_with('/') {
        Some(tty.to_owned())
    } else {
        Some(format!("/dev/{tty}"))
    }
}

/// An AppleScript string's contents: backslashes first, so the ones added
/// before quotes and newlines are not doubled again.
fn apple_script_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// The AppleScript that runs `command` in a new Terminal.app window and
/// brings it to the front (`MacBackend::terminalAppScript`).
pub fn terminal_app_script(command: &str) -> String {
    format!(
        "tell application \"Terminal\"\n    do script \"{}\"\n    activate\nend tell",
        apple_script_escape(command)
    )
}

/// AppleScript `display notification` (`MacBackend::notificationScript`).
pub fn notification_script(title: &str, body: &str) -> String {
    format!(
        "display notification \"{}\" with title \"{}\"",
        apple_script_escape(body),
        apple_script_escape(title)
    )
}

/// Whether Windows Terminal passes `value` on unchanged once it re-joins its
/// command line and expands `%NAME%` references in it.
fn survives_windows_terminal(value: &str) -> bool {
    if value.is_empty() || value.contains(['"', '\t', '\n', '%']) || value.contains("\\;") {
        return false;
    }
    !(value.contains(' ') && value.ends_with('\\'))
}

/// `;` separates `wt` subcommands; an escaped one reaches the program.
fn escape_subcommands(value: &str) -> String {
    value.replace(';', "\\;")
}

/// A Windows Terminal tab title: `Zerus: ` and the arguments on one line, at
/// most 100 UTF-16 units with an ellipsis.
pub fn windows_title(arguments: &[String]) -> String {
    let text = format!("Zerus: {}", arguments.join(" ")).replace('\n', " ");
    if text.encode_utf16().count() <= 100 {
        return text;
    }
    let mut units = 0;
    let mut kept: String = text
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 99
        })
        .collect();
    kept.push('\u{2026}');
    kept
}

/// `wt.exe` arguments that start `program` with exactly `arguments`
/// (`WindowsTerminalBackend::terminalArguments`); None when Windows Terminal
/// would change one of them.
pub fn terminal_arguments(
    mode: WindowsMode,
    title: &str,
    program: &str,
    arguments: &[String],
) -> Option<Vec<String>> {
    if !survives_windows_terminal(program)
        || !arguments
            .iter()
            .all(|argument| survives_windows_terminal(argument))
    {
        return None;
    }
    let window = if mode == WindowsMode::Window {
        "new"
    } else {
        "0"
    };
    let mut result = vec![
        "-w".to_owned(),
        window.to_owned(),
        "new-tab".to_owned(),
        "--title".to_owned(),
        escape_subcommands(title),
        "--".to_owned(),
        escape_subcommands(&program.replace('/', "\\")),
    ];
    result.extend(
        arguments
            .iter()
            .map(|argument| escape_subcommands(argument)),
    );
    Some(result)
}

/// `wt.exe` arguments that open `shell` (program first) in a tab starting in
/// `directory` (`WindowsTerminalBackend::shellArguments`); None when Windows
/// Terminal would change the folder or the shell's arguments.
pub fn shell_arguments(
    mode: WindowsMode,
    directory: &str,
    shell: &[String],
) -> Option<Vec<String>> {
    let folder = paths::native(directory, Style::Windows);
    let (program, arguments) = shell.split_first()?;
    if !survives_windows_terminal(&folder) {
        return None;
    }
    let mut result = terminal_arguments(
        mode,
        &windows_title(std::slice::from_ref(&folder)),
        program,
        arguments,
    )?;
    result.splice(3..3, ["-d".to_owned(), escape_subcommands(&folder)]);
    Some(result)
}

/// Windows PowerShell under `system_root` (`%SystemRoot%`, `C:\Windows` when
/// unset), with forward slashes as Qt names it.
pub fn windows_power_shell(system_root: Option<&str>) -> String {
    let root = system_root.unwrap_or("C:\\Windows").replace('\\', "/");
    format!("{root}/System32/WindowsPowerShell/v1.0/powershell.exe")
}

/// The PowerShell of a folder shell, program first: PowerShell 7 when
/// installed, otherwise Windows PowerShell without its banner.
pub fn windows_shell(pwsh: Option<&str>, system_root: Option<&str>) -> Vec<String> {
    match pwsh {
        Some(pwsh) => vec![pwsh.to_owned()],
        None => vec![windows_power_shell(system_root), "-NoLogo".to_owned()],
    }
}

/// Arguments of `hgs [@computer] account login ACCOUNT`.
pub fn sign_in_arguments(computer: &str, account: &str) -> Vec<String> {
    with_computer(computer, ["account", "login", account])
}

/// Arguments of `hgs [@computer] account install PROVIDER`.
pub fn install_arguments(computer: &str, provider: &str) -> Vec<String> {
    with_computer(computer, ["account", "install", provider])
}

/// Arguments of `hgs machine ssh ALIAS`.
pub fn ssh_arguments(alias: &str) -> Vec<String> {
    ["machine", "ssh", alias].map(str::to_owned).to_vec()
}

fn with_computer<const N: usize>(computer: &str, rest: [&str; N]) -> Vec<String> {
    let mut arguments = Vec::with_capacity(N + 1);
    if !computer.is_empty() {
        arguments.push(format!("@{computer}"));
    }
    arguments.extend(rest.map(str::to_owned));
    arguments
}

/// `QProcess::errorString()` after a failed start on Windows: `Process failed
/// to start:` and the system's text in the user's language, with Qt's
/// fallbacks when the system has none (`windowsErrorString`).
pub fn windows_start_error(code: u32, message: Option<&str>) -> String {
    const ERROR_MOD_NOT_FOUND: u32 = 126;
    let mut text = message.unwrap_or_default().to_owned();
    if text.is_empty() && code == ERROR_MOD_NOT_FOUND {
        text = "The specified module could not be found.".to_owned();
    }
    if let Some(stripped) = text.strip_suffix("\r\n") {
        text = stripped.to_owned();
    }
    if text.is_empty() {
        text = format!("Unknown error 0x{code:08x}.");
    }
    format!("Process failed to start: {text}")
}

/// How a started program gets its window on Windows; ignored elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// As `QProcess::startDetached`: a program with a window of its own.
    Plain,
    /// A console program in a new console window.
    Console,
    /// The console host, which opens its console itself, shown normally.
    Shown,
}

/// One program a launch starts, detached so it outlives the client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    pub program: String,
    pub arguments: Vec<String>,
    pub directory: String,
    pub window: Window,
}

/// The desktop a [`Launcher`] acts on.
pub trait Desktop: Send {
    /// The program's path on PATH (`QStandardPaths::findExecutable`).
    fn find(&self, program: &str) -> Option<String>;
    /// A non-empty environment variable.
    fn env(&self, name: &str) -> Option<String>;
    /// Whether something exists at `path`, links and app execution aliases
    /// included.
    fn exists(&self, path: &str) -> bool;
    /// The home folder, where terminals start.
    fn home(&self) -> String;
    /// This program, which runs Windows console windows with `--console-run`
    /// ([`console_host`]).
    fn console_host(&self) -> String;
    /// Starts a program detached; Err holds the system's reason.
    fn start(&mut self, start: &Start) -> Result<(), String>;
}

/// Konsole's D-Bus interface, as `KonsoleBackend` uses it. Errors are
/// `name: message`, or `no reply`.
pub trait Konsole: Send {
    /// `org.kde.konsole-PID` services, oldest process first.
    fn services(&mut self) -> Vec<String>;
    /// The numeric child nodes of `path` (`/Windows`, `/Sessions`), sorted.
    fn children(&mut self, service: &str, path: &str) -> Vec<i32>;
    /// A boolean property of `/konsole/MainWindow_WINDOW`; None when unread.
    fn window_flag(&mut self, service: &str, window: i32, property: &str) -> Option<bool>;
    fn new_session(&mut self, service: &str, window: i32) -> Result<i32, String>;
    fn send_text(&mut self, service: &str, session: i32, text: &str) -> Result<(), String>;
    fn run_command(&mut self, service: &str, session: i32, command: &str) -> Result<(), String>;
    fn set_current_session(&mut self, service: &str, window: i32, session: i32);
    /// Calls a widget method (`showNormal`, `raise`) of the main window.
    fn window_call(&mut self, service: &str, window: i32, method: &str);
    fn process_id(&mut self, service: &str, session: i32) -> Option<i32>;
    fn session_list(&mut self, service: &str, window: i32) -> Option<Vec<String>>;
    /// The pseudo-terminal of a process.
    fn tty_of(&mut self, pid: i32) -> Option<String> {
        tty_of_pid(pid)
    }
    /// Time for the compositor to activate a window.
    fn settle(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
}

/// No Konsole on the bus: launches open windows and nothing is raised.
#[derive(Debug, Default)]
pub struct NoKonsole;

impl Konsole for NoKonsole {
    fn services(&mut self) -> Vec<String> {
        Vec::new()
    }
    fn children(&mut self, _: &str, _: &str) -> Vec<i32> {
        Vec::new()
    }
    fn window_flag(&mut self, _: &str, _: i32, _: &str) -> Option<bool> {
        None
    }
    fn new_session(&mut self, _: &str, _: i32) -> Result<i32, String> {
        Err("no reply".to_owned())
    }
    fn send_text(&mut self, _: &str, _: i32, _: &str) -> Result<(), String> {
        Err("no reply".to_owned())
    }
    fn run_command(&mut self, _: &str, _: i32, _: &str) -> Result<(), String> {
        Err("no reply".to_owned())
    }
    fn set_current_session(&mut self, _: &str, _: i32, _: i32) {}
    fn window_call(&mut self, _: &str, _: i32, _: &str) {}
    fn process_id(&mut self, _: &str, _: i32) -> Option<i32> {
        None
    }
    fn session_list(&mut self, _: &str, _: i32) -> Option<Vec<String>> {
        None
    }
}

/// The Konsole of this computer: D-Bus with the `konsole` feature on Linux.
pub fn system_konsole() -> Box<dyn Konsole> {
    #[cfg(all(target_os = "linux", feature = "konsole"))]
    return Box::new(DbusKonsole::new());
    #[cfg(not(all(target_os = "linux", feature = "konsole")))]
    Box::new(NoKonsole)
}

/// How a session click ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Opened {
    /// The Konsole tab already attached to it came to the front.
    Raised,
    /// A terminal opened; `advisory` explains how it differs from the setting.
    Opened { advisory: Option<String> },
}

/// What runs a command line: the backends other than Windows Terminal.
enum Line<'a> {
    Command(&'a str),
    Konsole(KonsoleMode),
    TerminalApp,
}

/// Opens terminals for the launch actions with the current [`Settings`].
pub struct Launcher {
    settings: Settings,
    platform: Platform,
    desktop: Box<dyn Desktop>,
    konsole: Box<dyn Konsole>,
    /// The Konsole tab explanation went out for a session terminal.
    tab_advisory_shown: bool,
}

/// The `auto` mode's note that Konsole refuses tabs goes to the log once per
/// run.
static AUTO_WARNED: AtomicBool = AtomicBool::new(false);
/// A folder shell for a path that is no folder goes to the log once per run.
static FOLDER_WARNED: AtomicBool = AtomicBool::new(false);

/// Which terminal of the Qt GUI a launch stands for: the session terminal it
/// keeps for the whole run (session clicks and New session), or one it builds
/// for each other launch. Konsole explains a refused tab once per run in the
/// first, and every time in the second.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kept {
    Session,
    Fresh,
}

impl Launcher {
    /// A launcher for this computer.
    pub fn new(settings: Settings) -> Self {
        Self::with(
            settings,
            Platform::current(),
            Box::new(System),
            system_konsole(),
        )
    }

    pub fn with(
        settings: Settings,
        platform: Platform,
        desktop: Box<dyn Desktop>,
        konsole: Box<dyn Konsole>,
    ) -> Self {
        Self {
            settings,
            platform,
            desktop,
            konsole,
            tab_advisory_shown: false,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// Whether a session click copies its command instead of opening it.
    pub fn copies(&self) -> bool {
        self.settings.mode(self.platform) == "clipboard"
    }

    /// The terminal the next launch uses.
    pub fn backend(&self) -> Backend {
        let desktop = &self.desktop;
        launch_backend(&self.settings, self.platform, &|program| {
            desktop.find(program).is_some()
        })
    }

    /// Runs `program` with `arguments` in a terminal: agent sign-in and
    /// install, the SSH terminal. Ok holds an advisory when it opened other
    /// than asked; Err says why nothing opened.
    pub fn open_program(
        &mut self,
        program: &str,
        arguments: &[String],
    ) -> Result<Option<String>, String> {
        self.run_program(program, arguments, Kept::Fresh)
    }

    /// Runs a New session launch (`TrayAgent::launchSession`): as
    /// [`Launcher::open_program`], on the session terminal.
    pub fn open_new_session(
        &mut self,
        program: &str,
        arguments: &[String],
    ) -> Result<Option<String>, String> {
        let kept = if self.copies() {
            Kept::Fresh
        } else {
            Kept::Session
        };
        self.run_program(program, arguments, kept)
    }

    fn run_program(
        &mut self,
        program: &str,
        arguments: &[String],
        kept: Kept,
    ) -> Result<Option<String>, String> {
        match self.backend() {
            Backend::WindowsTerminal { mode } => {
                let title = windows_title(arguments);
                let (opened, advisory) = self.windows_terminal(
                    mode,
                    terminal_arguments(mode, &title, program, arguments),
                    "these arguments",
                    "the session",
                );
                if !opened {
                    self.console_host(&title, program, arguments, None)?;
                }
                Ok(advisory)
            }
            backend => {
                let command = shell::command(program, arguments, self.platform.shell());
                self.open_line(line(&backend), &command, kept)
            }
        }
    }

    /// An interactive shell in `directory`, a folder of this computer.
    pub fn open_shell(&mut self, directory: &str) -> Result<Option<String>, String> {
        let not_a_folder = || format!("not a folder of this computer: {directory}");
        match self.backend() {
            Backend::WindowsTerminal { mode } => {
                if !paths::absolute(directory, Style::Windows) {
                    return Err(not_a_folder());
                }
                let folder = paths::native(directory, Style::Windows);
                let pwsh = self.desktop.find("pwsh");
                let root = self.desktop.env("SystemRoot");
                let command = windows_shell(pwsh.as_deref(), root.as_deref());
                let (opened, advisory) = self.windows_terminal(
                    mode,
                    shell_arguments(mode, &folder, &command),
                    "this folder",
                    "the shell",
                );
                if !opened {
                    let title = windows_title(std::slice::from_ref(&folder));
                    self.console_host(&title, &command[0], &command[1..], Some(&folder))?;
                }
                Ok(advisory)
            }
            backend => {
                let command = shell::folder_shell("", "", directory, self.platform.shell())
                    .ok_or_else(not_a_folder)?;
                self.open_line(line(&backend), &command, Kept::Fresh)
            }
        }
    }

    /// A shell in `directory` of `computer` (empty for this one): a local
    /// shell there, or `hgs machine ssh` into a peer's folder. A path that is
    /// no folder of that computer opens nothing and reports nothing, as in Qt.
    pub fn open_folder_shell(
        &mut self,
        hgs: &str,
        computer: &str,
        directory: &str,
    ) -> Result<Option<String>, String> {
        if shell::folder_shell(hgs, computer, directory, self.platform.shell()).is_none() {
            if !FOLDER_WARNED.swap(true, Ordering::Relaxed) {
                let computer = if computer.is_empty() {
                    "this computer"
                } else {
                    computer
                };
                eprintln!("zerus: no folder shell: {directory:?} is not a folder of {computer}");
            }
            return Ok(None);
        }
        if computer.is_empty() {
            return self.open_shell(directory);
        }
        let arguments = ["machine", "ssh", computer, "--directory", directory].map(str::to_owned);
        self.open_program(hgs, &arguments)
    }

    /// Opens session `name` of `computer` (`openSessionTerminal`): the Konsole
    /// tab already attached on `tty` comes forward when it can, otherwise a
    /// terminal runs `hgs [@computer] a NAME`. In the clipboard mode the Qt
    /// GUI opens it on a terminal of its own.
    pub fn open_session(
        &mut self,
        hgs: &str,
        computer: &str,
        name: &str,
        tty: &str,
    ) -> Result<Opened, String> {
        let backend = self.backend();
        if !tty.is_empty()
            && matches!(backend, Backend::Konsole { .. })
            && !self.konsole.services().is_empty()
            && self.raise_tty(tty).is_ok()
        {
            return Ok(Opened::Raised);
        }
        let arguments = shell::open_arguments(computer, name, None);
        let program = if hgs.is_empty() { "hgs" } else { hgs };
        let kept = if self.copies() {
            Kept::Fresh
        } else {
            Kept::Session
        };
        self.run_program(program, &arguments, kept)
            .map(|advisory| Opened::Opened { advisory })
    }

    fn start(
        &mut self,
        program: &str,
        arguments: Vec<String>,
        window: Window,
    ) -> Result<(), String> {
        let start = Start {
            program: program.to_owned(),
            arguments,
            directory: self.desktop.home(),
            window,
        };
        self.desktop.start(&start)
    }

    fn open_line(
        &mut self,
        line: Line<'_>,
        command: &str,
        kept: Kept,
    ) -> Result<Option<String>, String> {
        match line {
            Line::Command(template) => {
                let mut argv = build_argv(template, command)?;
                let program = argv.remove(0);
                // A console program needs a console window of its own.
                let window = if self.platform == Platform::Windows {
                    Window::Console
                } else {
                    Window::Plain
                };
                self.start(&program, argv, window)
                    .map_err(|_| format!("cannot start '{program}'"))?;
                Ok(None)
            }
            Line::Konsole(mode) => self.konsole_open(mode, command, kept),
            Line::TerminalApp => {
                let arguments = vec!["-e".to_owned(), terminal_app_script(command)];
                self.start("osascript", arguments, Window::Plain)
                    .map_err(|_| "cannot start osascript".to_owned())?;
                Ok(None)
            }
        }
    }

    fn konsole_window(&mut self, command: &str) -> Result<(), String> {
        self.start("konsole", konsole_launch_args(command), Window::Plain)
            .map_err(|_| "cannot start konsole (not in PATH?)".to_owned())
    }

    /// `KonsoleBackend::open`: a tab in the active (else the oldest) Konsole
    /// window that accepts commands over D-Bus, otherwise a new window.
    fn konsole_open(
        &mut self,
        mode: KonsoleMode,
        command: &str,
        kept: Kept,
    ) -> Result<Option<String>, String> {
        if mode == KonsoleMode::Window {
            return self.konsole_window(command).map(|()| None);
        }
        let usable: Vec<String> = self
            .konsole
            .services()
            .into_iter()
            .filter(|service| input_allowed(self.konsole.as_mut(), service))
            .collect();
        if usable.is_empty() {
            self.konsole_window(command)?;
            if mode == KonsoleMode::Tab {
                if kept == Kept::Session {
                    if self.tab_advisory_shown {
                        return Ok(None);
                    }
                    self.tab_advisory_shown = true;
                }
                return Ok(Some(tab_advisory(&self.desktop.home())));
            }
            if !AUTO_WARNED.swap(true, Ordering::Relaxed) {
                eprintln!(
                    "zerus: Konsole will not take runCommand over D-Bus -- opening a separate \
                     window. A tab instead of a window is enabled by Settings -> Configure \
                     Konsole -> \"Enable the security sensitive parts of the DBus API\"."
                );
            }
            return Ok(None);
        }
        let Some((service, window)) = choose_window(self.konsole.as_mut(), &usable) else {
            return self.konsole_window(command).map(|()| None);
        };
        // The window may have closed since it was listed.
        let Ok(session) = self.konsole.new_session(&service, window) else {
            return self.konsole_window(command).map(|()| None);
        };
        // The tab exists now; a window on top of it would be one too many.
        if let Err(error) = self.konsole.run_command(&service, session, command) {
            return Err(format!("Konsole rejected the command ({error})"));
        }
        self.konsole.set_current_session(&service, window, session);
        bring_forward(self.konsole.as_mut(), &service, window);
        Ok(None)
    }

    /// `KonsoleBackend::raiseTty`: the Konsole tab whose shell owns `tty`
    /// comes forward; Ok only when its window really became active.
    fn raise_tty(&mut self, tty: &str) -> Result<(), String> {
        let wanted = normalize_tty(tty).ok_or_else(|| "empty tty".to_owned())?;
        let konsole = self.konsole.as_mut();
        for service in konsole.services() {
            for session in konsole.children(&service, "/Sessions") {
                let Some(pid) = konsole.process_id(&service, session) else {
                    continue;
                };
                // The tab's shell; the tmux client it starts shares its tty.
                if konsole.tty_of(pid).as_deref() != Some(wanted.as_str()) {
                    continue;
                }
                for window in konsole.children(&service, "/Windows") {
                    let Some(sessions) = konsole.session_list(&service, window) else {
                        continue;
                    };
                    if !sessions.contains(&session.to_string()) {
                        continue;
                    }
                    konsole.set_current_session(&service, window, session);
                    bring_forward(konsole, &service, window);
                    if window_is_active(konsole, &service, window) {
                        return Ok(());
                    }
                    return Err(format!(
                        "found the window with {wanted}, but the compositor would not raise it"
                    ));
                }
            }
        }
        Err(format!("no Konsole tab with {wanted}"))
    }

    /// Starts Windows Terminal unless the mode is `console`. When it cannot,
    /// false, with an advisory for an explicit tab or window.
    fn windows_terminal(
        &mut self,
        mode: WindowsMode,
        arguments: Option<Vec<String>>,
        refused: &str,
        what: &str,
    ) -> (bool, Option<String>) {
        if mode == WindowsMode::Console {
            return (false, None);
        }
        let reason = match (self.locate_windows_terminal(), arguments) {
            (None, _) => "Windows Terminal (wt.exe) is not installed".to_owned(),
            (Some(_), None) => format!("Windows Terminal cannot pass {refused} unchanged"),
            (Some(terminal), Some(arguments)) => {
                match self.start(&terminal, arguments, Window::Plain) {
                    Ok(()) => return (true, None),
                    Err(_) => format!(
                        "Windows Terminal ({}) did not start",
                        terminal.replace('/', "\\")
                    ),
                }
            }
        };
        let advisory = (mode != WindowsMode::Auto).then(|| {
            format!(
                "{reason}, so {what} opened in a console window. Choose Console in \
                 \"Open in\" to use console windows deliberately."
            )
        });
        (false, advisory)
    }

    /// `wt.exe`: on PATH, or the app execution alias in the user's
    /// WindowsApps folder, which PATH may lack.
    fn locate_windows_terminal(&self) -> Option<String> {
        if let Some(found) = self.desktop.find("wt") {
            return Some(found);
        }
        let local = self.desktop.env("LOCALAPPDATA")?;
        let alias = format!("{}/Microsoft/WindowsApps/wt.exe", local.replace('\\', "/"));
        self.desktop.exists(&alias).then_some(alias)
    }

    /// A console window run by this program (`--console-run`), in `directory`
    /// or the home folder.
    fn console_host(
        &mut self,
        title: &str,
        program: &str,
        arguments: &[String],
        directory: Option<&str>,
    ) -> Result<(), String> {
        let host = self.desktop.console_host();
        let start = Start {
            program: host.clone(),
            arguments: console_host::arguments(title, program, arguments),
            directory: directory.map_or_else(|| self.desktop.home(), str::to_owned),
            window: Window::Shown,
        };
        self.desktop
            .start(&start)
            .map_err(|error| format!("cannot start {}: {error}", host.replace('/', "\\")))
    }
}

fn line(backend: &Backend) -> Line<'_> {
    match backend {
        Backend::Command { template } => Line::Command(template),
        Backend::Konsole { mode } => Line::Konsole(*mode),
        Backend::TerminalApp => Line::TerminalApp,
        // Windows Terminal starts programs itself; its callers handle it.
        Backend::WindowsTerminal { .. } => Line::Command(""),
    }
}

/// The Konsole tab explanation (`KonsoleBackend::open` in tab mode).
fn tab_advisory(home: &str) -> String {
    format!(
        "Konsole will not hand out a tab over D-Bus -- the API is off in its settings. \
         To make a click open a tab instead of a window: add \
         EnableSecuritySensitiveDBusAPI=true under [KonsoleWindow] in {home}/.config/konsolerc \
         (or Settings -> Configure Konsole -> \"Enable the security sensitive parts of the \
         DBus API\") and restart Konsole. The session was opened in a separate window."
    )
}

/// Whether Konsole accepts input over D-Bus: an empty `sendText` to a session
/// it has, which types nothing. Checked before a tab is created, so a refusal
/// leaves no empty tab behind.
fn input_allowed(konsole: &mut dyn Konsole, service: &str) -> bool {
    let sessions = konsole.children(service, "/Sessions");
    match sessions.first() {
        Some(&session) => konsole.send_text(service, session, "").is_ok(),
        None => false,
    }
}

/// The window for a new tab: the active one, else the oldest process's first
/// window, so tabs gather in one place.
fn choose_window(konsole: &mut dyn Konsole, services: &[String]) -> Option<(String, i32)> {
    let mut first = None;
    for service in services {
        for window in konsole.children(service, "/Windows") {
            if first.is_none() {
                first = Some((service.clone(), window));
            }
            if konsole.window_flag(service, window, "isActiveWindow") == Some(true) {
                return Some((service.clone(), window));
            }
        }
    }
    first
}

/// What a compositor allows without an activation token: restore a minimized
/// window, and ask for a raise.
fn bring_forward(konsole: &mut dyn Konsole, service: &str, window: i32) {
    if konsole.window_flag(service, window, "minimized") == Some(true) {
        konsole.window_call(service, window, "showNormal");
    }
    konsole.window_call(service, window, "raise");
}

fn window_is_active(konsole: &mut dyn Konsole, service: &str, window: i32) -> bool {
    if konsole.window_flag(service, window, "isActiveWindow") == Some(true) {
        return true;
    }
    konsole.settle();
    konsole.window_flag(service, window, "isActiveWindow") == Some(true)
}
