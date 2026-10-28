//! The menu-bar item's data and actions. The pump taps every change into a
//! thread that keeps a [`Desk`] and, at most every [`PACE`], offers the
//! status item its face, tooltip and menu on the main thread. Menu actions
//! open the window, open a running session in Terminal.app or copy its
//! command (Qt's "Open in"), open a saved session's row, or quit. A session
//! or notice the webview cannot take yet waits in [`Pending`] until it asks.
//!
//! [`Bar`] is the status item's state apart from AppKit: what reaches it
//! ([`Gate`]), the Dock badge, the menus that are tracking, and the action
//! each item of the menu names.
// The menu bar exists only on macOS in this phase; Linux keeps the Qt GUI.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use zerus_client_core::menubar::{tray_conf_value, Apply, Desk, Entry, Face, Gate, OpenMode};
use zerus_client_core::shell;
use zerus_client_core::{Change, ComputerId};

/// The status item is offered at most this often.
pub const PACE: Duration = Duration::from_millis(250);
/// The client's own "Open in" choice, in its data directory.
pub const MODE_FILE: &str = "menubar.json";
/// At most this many notices wait for the webview.
pub const PENDING_NOTICES: usize = 8;

pub enum DeskInput {
    Change(Box<Change>),
    Mode(OpenMode),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSession {
    pub computer: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub title: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub open: Option<OpenSession>,
    pub notices: Vec<Notice>,
}

impl Pending {
    pub fn notice(&mut self, notice: Notice) {
        if self.notices.len() == PENDING_NOTICES {
            self.notices.remove(0);
        }
        self.notices.push(notice);
    }
}

/// What the menu bar and notifications send the window's page. Until the
/// page asks ([`desk_take_pending`]) it waits in [`Pending`]; afterwards it
/// goes out as an event, until the page loads again. Each page load has a
/// number, which the page passes when it asks, so a late request from the
/// page before a reload takes nothing.
#[derive(Debug, Default)]
pub struct Inbox {
    page: u64,
    ready: bool,
    pending: Pending,
}

impl Inbox {
    /// A session to open: `send` gets it now if the page listens, else it
    /// waits. The glue calls this on the main thread and sends under the lock
    /// that [`Inbox::loading`] takes too, so a page that starts loading cannot
    /// fall between the decision and the event.
    pub fn open(&mut self, target: OpenSession, send: impl FnOnce(OpenSession)) {
        if self.ready {
            send(target);
        } else {
            self.pending.open = Some(target);
        }
    }

    /// A notice: `send` gets it now if the page listens, else it waits.
    pub fn notice(&mut self, notice: Notice, send: impl FnOnce(Notice)) {
        if self.ready {
            send(notice);
        } else {
            self.pending.notice(notice);
        }
    }

    /// The page of load `page` asks for what waited and listens from now on.
    /// A request from an older page, which arrived after a reload started,
    /// takes nothing.
    pub fn take(&mut self, page: u64) -> Pending {
        if page != self.page {
            return Pending::default();
        }
        self.ready = true;
        std::mem::take(&mut self.pending)
    }

    /// A page starts loading (again): nothing listens until it asks. Returns
    /// its number.
    pub fn loading(&mut self) -> u64 {
        self.page += 1;
        self.ready = false;
        self.page
    }

    /// The number of the page loading or loaded now.
    pub fn page(&self) -> u64 {
        self.page
    }
}

/// Kept in the app state.
pub struct DeskState {
    sender: Mutex<Sender<DeskInput>>,
    mode: Mutex<OpenMode>,
    dir: PathBuf,
    inbox: Mutex<Inbox>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Serialize, Deserialize)]
struct Saved {
    open_mode: OpenMode,
}

/// The "Open in" choice: the client's own file, else `open-mode` from Qt's
/// `tray.conf` (read only), else Terminal.app (Decision 10).
pub fn load_mode(dir: &Path, tray_conf: Option<&str>) -> OpenMode {
    std::fs::read(dir.join(MODE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
        .map(|saved| saved.open_mode)
        .or_else(|| {
            tray_conf
                .and_then(|text| tray_conf_value(text, "open-mode"))
                .and_then(|value| OpenMode::from_config(&value))
        })
        .unwrap_or_default()
}

pub fn save_mode(dir: &Path, mode: OpenMode) -> std::io::Result<()> {
    write_private(
        &dir.join(MODE_FILE),
        &serde_json::to_vec(&Saved { open_mode: mode })?,
    )
}

/// Writes a private (0600) file through a temporary file and a rename.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = PathBuf::from(temporary);
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

/// `~/.config/hgs/tray.conf` (`AppConfig::configPath`), read only.
pub fn tray_conf() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    std::fs::read_to_string(Path::new(&home).join(".config/hgs/tray.conf")).ok()
}

/// The changes the menu bar reads.
pub fn menu_bar_change(change: &Change) -> bool {
    matches!(
        change,
        Change::Upsert { .. }
            | Change::Remove { .. }
            | Change::Replace { .. }
            | Change::Computer { .. }
            | Change::Machine { .. }
    )
}

/// Taps the pump and starts the desk thread. Call before the fleet starts,
/// so the first catalog reaches the menu bar.
pub fn start(app: &AppHandle, pump: &crate::pump::Pump, dir: PathBuf, mode: OpenMode) {
    let (sender, inputs) = mpsc::channel();
    let tap = sender.clone();
    pump.tap(Box::new(move |change: &Change| {
        if menu_bar_change(change) {
            let _ = tap.send(DeskInput::Change(Box::new(change.clone())));
        }
    }));
    app.manage(DeskState {
        sender: Mutex::new(sender),
        mode: Mutex::new(mode),
        dir,
        inbox: Mutex::new(Inbox::default()),
    });
    let handle = app.clone();
    std::thread::Builder::new()
        .name("zerus-client-desk".into())
        .spawn(move || run(handle, inputs, mode))
        .expect("the desk thread starts");
}

fn run(app: AppHandle, inputs: Receiver<DeskInput>, mut mode: OpenMode) {
    let mut desk = Desk::default();
    // The menu with Open Zerus and Quit, before any change arrives or when the
    // fleet does not start: Qt builds its menu at once (`TrayAgent.cpp:252-254`).
    publish(&app, &desk, mode);
    while let Ok(first) = inputs.recv() {
        for input in std::iter::once(first).chain(std::iter::from_fn(|| inputs.try_recv().ok())) {
            match input {
                DeskInput::Change(change) => {
                    desk.observe(&change);
                }
                DeskInput::Mode(chosen) => {
                    mode = chosen;
                    store_mode(&app, chosen);
                }
            }
        }
        publish(&app, &desk, mode);
        std::thread::sleep(PACE);
    }
}

fn publish(app: &AppHandle, desk: &Desk, mode: OpenMode) {
    #[cfg(target_os = "macos")]
    {
        let (face, tooltip, menu, badge) =
            (desk.face(), desk.tooltip(), desk.menu(mode), desk.badge());
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            crate::platform::macos_menubar::offer(&handle, face, tooltip, menu, badge)
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, desk, mode);
}

/// What the webview could not take yet; its page asks once, with the number
/// [`page_loaded`] gave it.
#[tauri::command]
pub fn desk_take_pending(app: AppHandle, page: u64) -> Pending {
    app.try_state::<DeskState>()
        .map(|state| lock(&state.inbox).take(page))
        .unwrap_or_default()
}

/// The main window's page started to load (on the main thread): it gets a
/// new number, and what follows waits until it asks.
pub fn page_loading(app: &AppHandle) {
    if let Some(state) = app.try_state::<DeskState>() {
        lock(&state.inbox).loading();
    }
}

/// The main window's page has loaded (on the main thread): tells it its
/// number. Only the new document runs a script at this point, so a page
/// that is going away never learns it.
pub fn page_loaded(webview: &tauri::Webview) {
    let Some(state) = webview.app_handle().try_state::<DeskState>() else {
        return;
    };
    let page = lock(&state.inbox).page();
    let script = format!("window.__zerusDeskPage = {page}; window.__zerusDeskPageReady?.({page});");
    if let Err(error) = webview.eval(script) {
        eprintln!("zerus-client: could not tell the page its number: {error}");
    }
}

/// Opens the window on a session's row (`TrayAgent.cpp:214-216`).
pub fn open_session(app: &AppHandle, computer: &ComputerId, key: &str) {
    let target = OpenSession {
        computer: computer.0.clone(),
        key: key.to_owned(),
    };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(error) = crate::lifecycle::show_main(&handle) {
            eprintln!("zerus-client: could not open the window: {error}");
        }
        let send = |target: OpenSession| {
            let _ = handle.emit("open-session", target);
        };
        // Decided and sent under the inbox lock, on the main thread like a page load.
        match handle.try_state::<DeskState>() {
            Some(state) => lock(&state.inbox).open(target, send),
            None => send(target),
        }
    });
}

/// A notice inside the window (Qt's `showNotificationNotice`).
#[allow(dead_code)] // Task 6's notifier is its first caller.
pub fn window_notice(app: &AppHandle, title: &str, text: &str) {
    let notice = Notice {
        title: title.to_owned(),
        text: text.to_owned(),
    };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let send = |notice: Notice| {
            let _ = handle.emit("desk-notice", notice);
        };
        // Decided and sent under the inbox lock, on the main thread like a page load.
        match handle.try_state::<DeskState>() {
            Some(state) => lock(&state.inbox).notice(notice, send),
            None => send(notice),
        }
    });
}

/// Qt's `TrayAgent::notify` on macOS: `osascript display notification`,
/// shown at once without the app's own permission (`TrayAgent.cpp:796-830`).
pub fn tray_notify(title: &str, body: &str) {
    let spawned = std::process::Command::new("osascript")
        .arg("-e")
        .arg(shell::notification_script(title, body))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(error) => eprintln!("zerus-client: {title}: {body} ({error})"),
    }
}

/// A running session's entry: Terminal.app or the clipboard, as "Open in"
/// says (`TrayAgent::activateSessionWith`, `TrayAgent.cpp:631-659`). A copy
/// is confirmed every time (`ClipboardBackend::open`); a failure of either
/// shows the command to run by hand.
pub fn open_running(app: &AppHandle, computer: &ComputerId, name: &str) {
    let hgs = zerus_client_core::default_hgs();
    let command = shell::open_command(
        &hgs.to_string_lossy(),
        &computer.0,
        name,
        None,
        shell::current_shell(),
    );
    let mode = app
        .try_state::<DeskState>()
        .map(|state| *lock(&state.mode))
        .unwrap_or_default();
    let opened = match mode {
        OpenMode::Terminal => open_in_terminal(&command),
        OpenMode::Clipboard => {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            shell::copyable(&command)
                .and_then(|()| {
                    app.clipboard()
                        .write_text(command.clone())
                        .map_err(|error| {
                            format!("cannot put the command on the clipboard ({error})")
                        })
                })
                .map(|()| {
                    tray_notify(
                        "Zerus: copied to the clipboard",
                        &shell::notification_text(&command),
                    )
                })
        }
    };
    if let Err(error) = opened {
        tray_notify(
            "Zerus: the terminal did not open",
            &format!("{error}\n\nCommand: {command}\nRun it in a terminal by hand."),
        );
    }
}

/// `osascript` with Qt's Terminal.app script, detached (`MacBackend::open`).
fn open_in_terminal(command: &str) -> Result<(), String> {
    detached_script("osascript", &shell::terminal_app_script(command))
}

/// Starts `program -e script` from HOME without waiting for it. Qt reports
/// only that it could not start (`MacBackend.cpp:61-65`); the reason goes to
/// the log.
fn detached_script(program: &str, script: &str) -> Result<(), String> {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/".into());
    let spawned = std::process::Command::new(program)
        .arg("-e")
        .arg(script)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        Err(error) => {
            eprintln!("zerus-client: {program}: {error}");
            Err("cannot start osascript".to_owned())
        }
    }
}

/// Takes `chosen` unless it is the mode already in use (`TrayAgent::setOpenMode`
/// returns early on the checked entry). Returns whether it changed.
fn choose(current: &mut OpenMode, chosen: OpenMode) -> bool {
    if *current == chosen {
        return false;
    }
    *current = chosen;
    true
}

/// The "Open in" choice: applied at once (`TrayAgent::setOpenMode`). The desk
/// thread saves it, so the main thread never waits for the disk.
pub fn set_mode(app: &AppHandle, mode: OpenMode) {
    let Some(state) = app.try_state::<DeskState>() else {
        return;
    };
    if !choose(&mut lock(&state.mode), mode) {
        return;
    }
    let _ = lock(&state.sender).send(DeskInput::Mode(mode));
}

/// Saves a chosen mode; Qt's notice when it cannot (`TrayAgent.cpp:864-872`).
fn mode_saved(dir: &Path, mode: OpenMode) -> Result<(), Notice> {
    save_mode(dir, mode).map_err(|error| Notice {
        title: "Zerus: mode applied, but not saved".to_owned(),
        text: format!(
            "New sessions already open the new way, but the choice could not be written to {}: {error}",
            dir.join(MODE_FILE).display()
        ),
    })
}

/// On the desk thread: saves the mode `set_mode` applied.
fn store_mode(app: &AppHandle, mode: OpenMode) {
    let Some(dir) = app.try_state::<DeskState>().map(|state| state.dir.clone()) else {
        return;
    };
    if let Err(notice) = mode_saved(&dir, mode) {
        tray_notify(&notice.title, &notice.text);
    }
}

/// What a menu entry does when chosen (Qt's `TrayMenu` signals).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// "Open Zerus" (`sessionsRequested`).
    Open,
    /// A running session opens or is copied; a saved one opens its row.
    Session {
        computer: ComputerId,
        key: String,
        name: String,
        running: bool,
    },
    /// An "Open in" mode (`openModeChosen`).
    Mode(OpenMode),
    Quit,
}

/// One native item of the menu, with the id its click reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// An entry that can be chosen.
    Action {
        id: String,
        text: String,
    },
    /// A line that cannot be chosen: a computer's header or its state.
    Label {
        id: String,
        text: String,
    },
    Separator,
    /// The "Open in" submenu: each mode's id, the mode and whether it is checked.
    OpenIn {
        id: String,
        modes: Vec<(String, OpenMode, bool)>,
    },
}

/// What one offer changes.
#[derive(Debug, Default, PartialEq)]
pub struct Offered {
    /// The Dock badge to set, when it changed.
    pub badge: Option<Option<String>>,
    /// The status item's changes the gate lets through.
    pub changes: Vec<Apply>,
}

/// The status item's state apart from AppKit; the glue keeps it on the main
/// thread.
#[derive(Debug, Default)]
pub struct Bar {
    gate: Gate,
    /// The Dock badge last set; None before the first offer.
    badge: Option<Option<String>>,
    /// Menus of the app that are tracking now.
    tracking: usize,
    /// Each layout's ids are new, so a click never names another entry.
    generation: u64,
    /// The actions of the latest layout.
    actions: Arc<HashMap<String, MenuAction>>,
    /// The actions of the menu shown last: a click arrives after the menu
    /// closed, when a pending change may have rebuilt it.
    shown: Arc<HashMap<String, MenuAction>>,
}

impl Bar {
    /// The desk thread's latest state. The Dock badge is not the status item:
    /// it follows attention while a menu is open, as Qt sets it before its
    /// quiet tray icon decides (`TrayAgent::refreshIcon`).
    pub fn offer(
        &mut self,
        face: Face,
        tooltip: String,
        menu: Vec<Entry>,
        badge: Option<String>,
    ) -> Offered {
        let badge = (self.badge.as_ref() != Some(&badge)).then(|| {
            self.badge = Some(badge.clone());
            badge
        });
        Offered {
            badge,
            changes: self.gate.offer(face, tooltip, menu),
        }
    }

    /// A menu of the app began tracking (`NSMenuDidBeginTrackingNotification`):
    /// nothing reaches the status item until every menu closed (`QuietTrayIcon.h`).
    pub fn began(&mut self) {
        self.tracking += 1;
        self.shown = Arc::clone(&self.actions);
        self.gate.menu_opened();
    }

    /// A menu ended tracking. True when none tracks any more: the caller then
    /// settles once the tracking loop has unwound.
    pub fn ended(&mut self) -> bool {
        self.tracking = self.tracking.saturating_sub(1);
        self.tracking == 0
    }

    /// What waited while the menus were open, unless another one opened since.
    pub fn settle(&mut self) -> Vec<Apply> {
        if self.tracking == 0 && self.gate.is_open() {
            self.gate.menu_closed()
        } else {
            Vec::new()
        }
    }

    /// No menu tracks and nothing waits for one to close: the status item may change.
    pub fn quiet(&self) -> bool {
        self.tracking == 0 && !self.gate.is_open()
    }

    /// The native items of a new menu, Qt's texts (`TrayMenu::rebuild`).
    pub fn layout(&mut self, entries: &[Entry]) -> Vec<Item> {
        self.generation += 1;
        let mut actions = HashMap::new();
        let mut items = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let id = format!("zerus-{}-{index}", self.generation);
            let mut chosen_by = |text: &str, action: MenuAction| {
                actions.insert(id.clone(), action);
                Item::Action {
                    id: id.clone(),
                    text: text.to_owned(),
                }
            };
            items.push(match entry {
                Entry::Open => chosen_by("Open Zerus", MenuAction::Open),
                Entry::Separator => Item::Separator,
                Entry::Label(text) => Item::Label {
                    id: id.clone(),
                    text: text.clone(),
                },
                Entry::Session {
                    computer,
                    key,
                    name,
                    text,
                    running,
                } => chosen_by(
                    text,
                    MenuAction::Session {
                        computer: computer.clone(),
                        key: key.clone(),
                        name: name.clone(),
                        running: *running,
                    },
                ),
                Entry::OpenIn(chosen) => {
                    let modes = OpenMode::ALL
                        .into_iter()
                        .map(|mode| {
                            let item = format!("{id}-{}", mode.label());
                            actions.insert(item.clone(), MenuAction::Mode(mode));
                            (item, mode, mode == *chosen)
                        })
                        .collect();
                    Item::OpenIn {
                        id: id.clone(),
                        modes,
                    }
                }
                Entry::Quit => chosen_by("Quit", MenuAction::Quit),
            });
        }
        self.actions = Arc::new(actions);
        items
    }

    /// The action of a clicked item, of the latest menu or the one shown.
    pub fn action(&self, id: &str) -> Option<MenuAction> {
        self.actions.get(id).or_else(|| self.shown.get(id)).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zcd-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_open_mode_prefers_the_clients_file_then_tray_conf() {
        let dir = scratch("mode");
        assert_eq!(load_mode(&dir, None), OpenMode::Terminal);
        assert_eq!(
            load_mode(&dir, Some("open-mode=clipboard\n")),
            OpenMode::Clipboard
        );
        assert_eq!(
            load_mode(&dir, Some("open-mode=tab\n")),
            OpenMode::Terminal,
            "a Linux mode is not a macOS mode"
        );
        save_mode(&dir, OpenMode::Terminal).unwrap();
        assert_eq!(
            load_mode(&dir, Some("open-mode=clipboard\n")),
            OpenMode::Terminal
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(MODE_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_notices_are_bounded() {
        let mut pending = Pending::default();
        for index in 0..10 {
            pending.notice(Notice {
                title: "Zerus".into(),
                text: index.to_string(),
            });
        }
        assert_eq!(pending.notices.len(), PENDING_NOTICES);
        assert_eq!(pending.notices[0].text, "2");
    }

    #[test]
    fn the_desk_listens_to_rows_statuses_and_cards_only() {
        use zerus_client_core::{
            ActivityUpdate, ComputerStatus, MachineMeta, OrganizationUpdate, ProcessesUpdate,
            TasksUpdate, ViewRow,
        };
        let local = ComputerId::local;
        let read = [
            Change::Upsert {
                computer: local(),
                key: "k".into(),
                row: ViewRow::default(),
            },
            Change::Remove {
                computer: local(),
                key: "k".into(),
                reason: None,
            },
            Change::Replace {
                computer: local(),
                rows: Vec::new(),
            },
            Change::Computer {
                computer: local(),
                status: ComputerStatus::Live,
            },
            Change::Machine {
                computer: local(),
                machine: MachineMeta::default(),
            },
        ];
        for change in &read {
            assert!(menu_bar_change(change), "{change:?}");
        }
        let skipped = [
            Change::Activity {
                computer: local(),
                key: "k".into(),
                update: ActivityUpdate::Ended,
            },
            Change::Processes {
                computer: local(),
                key: "k".into(),
                update: ProcessesUpdate::Ended,
            },
            Change::Tasks {
                computer: local(),
                key: "k".into(),
                update: TasksUpdate::Ended,
            },
            Change::Organization {
                computer: local(),
                update: OrganizationUpdate::Ended,
            },
        ];
        for change in &skipped {
            assert!(!menu_bar_change(change), "{change:?}");
        }
    }

    #[test]
    fn a_mode_that_cannot_be_saved_gives_qts_notice() {
        let dir = scratch("unsaved");
        assert_eq!(mode_saved(&dir, OpenMode::Clipboard), Ok(()));
        assert_eq!(load_mode(&dir, None), OpenMode::Clipboard);
        // A file where the data directory should be.
        let file = dir.join("not-a-directory");
        std::fs::write(&file, b"").unwrap();
        let notice = mode_saved(&file, OpenMode::Terminal).unwrap_err();
        assert_eq!(notice.title, "Zerus: mode applied, but not saved");
        assert!(
            notice.text.starts_with(&format!(
                "New sessions already open the new way, but the choice could not be written to {}: ",
                file.join(MODE_FILE).display()
            )),
            "{}",
            notice.text
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_osascript_that_cannot_start_reads_like_qt() {
        assert_eq!(
            detached_script("/nonexistent/zerus-osascript", "return"),
            Err("cannot start osascript".to_owned())
        );
    }

    fn target(key: &str) -> OpenSession {
        OpenSession {
            computer: "studio".into(),
            key: key.into(),
        }
    }

    /// What an inbox call sent at once.
    fn sent<T>(call: impl FnOnce(&mut dyn FnMut(T))) -> Vec<T> {
        let mut out = Vec::new();
        call(&mut |value| out.push(value));
        out
    }

    #[test]
    fn a_session_waits_until_the_page_takes_it() {
        let mut inbox = Inbox::default();
        let page = inbox.loading();
        assert!(
            sent(|send| inbox.open(target("a"), send)).is_empty(),
            "no page yet: held"
        );
        let notice = Notice {
            title: "Zerus".into(),
            text: "sample".into(),
        };
        assert!(sent(|send| inbox.notice(notice.clone(), send)).is_empty());
        let pending = inbox.take(page);
        assert_eq!(pending.open, Some(target("a")));
        assert_eq!(pending.notices, std::slice::from_ref(&notice));
        assert_eq!(inbox.take(page), Pending::default(), "taken once");
        assert_eq!(
            sent(|send| inbox.open(target("b"), send)),
            [target("b")],
            "a ready page gets the event"
        );
        assert_eq!(sent(|send| inbox.notice(notice.clone(), send)), [notice]);
        // A window that exists but whose page loads again (a reload, a new webview).
        let next = inbox.loading();
        assert!(sent(|send| inbox.open(target("c"), send)).is_empty());
        assert!(
            sent(|send| inbox.open(target("d"), send)).is_empty(),
            "the latest session wins"
        );
        assert_eq!(inbox.take(next).open, Some(target("d")));
    }

    #[test]
    fn a_page_that_starts_loading_holds_what_follows() {
        // The decision and the send happen in one call, under the lock the page
        // load also takes, so nothing is sent to a page that started loading.
        let mut inbox = Inbox::default();
        let first = inbox.loading();
        inbox.take(first);
        inbox.loading();
        let mut sent_after_load = Vec::new();
        inbox.open(target("a"), &mut |value| sent_after_load.push(value));
        assert!(
            sent_after_load.is_empty(),
            "the reloading page would lose it"
        );
        assert_eq!(inbox.pending.open, Some(target("a")));
    }

    #[test]
    fn a_take_from_the_page_before_a_reload_is_ignored() {
        let mut inbox = Inbox::default();
        let old = inbox.loading();
        inbox.take(old);
        // The old page asks again, but its request arrives after the reload started.
        let new = inbox.loading();
        assert!(sent(|send| inbox.open(target("a"), send)).is_empty());
        assert_eq!(
            inbox.take(old),
            Pending::default(),
            "a stale take gets nothing"
        );
        assert!(
            sent(|send| inbox.open(target("b"), send)).is_empty(),
            "and does not make the new page ready"
        );
        assert_eq!(inbox.take(new).open, Some(target("b")));
        assert_eq!(inbox.page(), new);
    }

    #[test]
    fn choosing_the_mode_already_chosen_changes_nothing() {
        // `TrayAgent::setOpenMode` returns early: no new backend, no write.
        let mut mode = OpenMode::Terminal;
        assert!(!choose(&mut mode, OpenMode::Terminal));
        assert!(choose(&mut mode, OpenMode::Clipboard));
        assert_eq!(mode, OpenMode::Clipboard);
    }

    fn offer(bar: &mut Bar, total: usize, badge: Option<&str>) -> Offered {
        bar.offer(
            Face::counts(total, 0, false),
            format!("{total} sessions"),
            vec![Entry::Open, Entry::Quit],
            badge.map(str::to_owned),
        )
    }

    fn faces(changes: &[Apply]) -> Vec<Face> {
        changes
            .iter()
            .filter_map(|change| match change {
                Apply::Face(face) => Some(face.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_status_item_waits_while_any_menu_tracks() {
        let mut bar = Bar::default();
        assert_eq!(
            faces(&offer(&mut bar, 1, None).changes),
            [Face::counts(1, 0, false)]
        );
        assert!(bar.quiet());
        bar.began();
        // "Open in" or another menu of the app nests inside the first.
        bar.began();
        assert!(
            offer(&mut bar, 2, None).changes.is_empty(),
            "nothing reaches the status item while a menu tracks"
        );
        assert!(
            !bar.ended(),
            "an inner menu closed; the outer one still tracks"
        );
        assert!(offer(&mut bar, 3, None).changes.is_empty());
        assert!(bar.ended(), "the last menu closed");
        assert!(!bar.quiet(), "the status item waits for the settle");
        assert_eq!(
            faces(&bar.settle()),
            [Face::counts(3, 0, false)],
            "the latest offer shows once the menus closed"
        );
        assert!(bar.quiet());
        assert!(bar.settle().is_empty());
        assert!(!offer(&mut bar, 4, None).changes.is_empty());
    }

    #[test]
    fn a_menu_opened_before_the_close_settles_keeps_the_status_item_waiting() {
        let mut bar = Bar::default();
        offer(&mut bar, 1, None);
        bar.began();
        offer(&mut bar, 2, None);
        assert!(bar.ended());
        // The settle runs after the tracking loop unwound; a new menu came first.
        bar.began();
        assert!(bar.settle().is_empty(), "the reopened menu still tracks");
        assert!(bar.ended());
        assert_eq!(faces(&bar.settle()), [Face::counts(2, 0, false)]);
    }

    #[test]
    fn the_dock_badge_follows_attention_while_a_menu_is_open() {
        // `TrayAgent::refreshIcon` sets the badge before the quiet tray icon
        // decides; only the status item waits for the menu.
        let mut bar = Bar::default();
        assert_eq!(
            offer(&mut bar, 1, None).badge,
            Some(None),
            "the first offer sets the badge"
        );
        bar.began();
        let offered = offer(&mut bar, 2, Some("3"));
        assert_eq!(offered.badge, Some(Some("3".to_owned())));
        assert!(offered.changes.is_empty());
        assert_eq!(
            offer(&mut bar, 2, Some("3")).badge,
            None,
            "an unchanged badge is not set again"
        );
        assert_eq!(offer(&mut bar, 2, None).badge, Some(None));
    }

    fn session(key: &str, name: &str, running: bool) -> Entry {
        Entry::Session {
            computer: ComputerId::from("studio"),
            key: key.into(),
            name: name.into(),
            text: name.into(),
            running,
        }
    }

    fn id(item: &Item) -> &str {
        match item {
            Item::Action { id, .. } | Item::Label { id, .. } | Item::OpenIn { id, .. } => id,
            Item::Separator => "",
        }
    }

    #[test]
    fn the_menu_follows_qt_and_each_entry_names_its_action() {
        let mut bar = Bar::default();
        let items = bar.layout(&[
            Entry::Open,
            Entry::Separator,
            Entry::Label("studio".into()),
            session("run:a", "claude/sample-project/a", true),
            session("run:b", "codex/sample-project/b", false),
            Entry::Separator,
            Entry::OpenIn(OpenMode::Clipboard),
            Entry::Quit,
        ]);
        let texts: Vec<&str> = items
            .iter()
            .map(|item| match item {
                Item::Action { text, .. } | Item::Label { text, .. } => text.as_str(),
                Item::OpenIn { .. } => "Open in",
                Item::Separator => "-",
            })
            .collect();
        assert_eq!(
            texts,
            [
                "Open Zerus",
                "-",
                "studio",
                "claude/sample-project/a",
                "codex/sample-project/b",
                "-",
                "Open in",
                "Quit"
            ]
        );
        assert!(
            matches!(items[2], Item::Label { .. }),
            "a header cannot be chosen"
        );
        assert_eq!(bar.action(id(&items[0])), Some(MenuAction::Open));
        assert_eq!(
            bar.action(id(&items[3])),
            Some(MenuAction::Session {
                computer: ComputerId::from("studio"),
                key: "run:a".into(),
                name: "claude/sample-project/a".into(),
                running: true,
            })
        );
        assert!(matches!(
            bar.action(id(&items[4])),
            Some(MenuAction::Session { running: false, .. })
        ));
        assert_eq!(bar.action(id(&items[2])), None);
        let Item::OpenIn { modes, .. } = &items[6] else {
            panic!("Open in is a submenu")
        };
        let shown: Vec<(&str, bool)> = modes
            .iter()
            .map(|(_, mode, checked)| (mode.label(), *checked))
            .collect();
        assert_eq!(
            shown,
            [("Terminal.app", false), ("Clipboard", true)],
            "Qt's macOS modes, the chosen one checked"
        );
        for (id, mode, _) in modes {
            assert_eq!(bar.action(id), Some(MenuAction::Mode(*mode)));
        }
        assert_eq!(bar.action(id(&items[7])), Some(MenuAction::Quit));
    }

    #[test]
    fn a_click_names_what_the_shown_menu_showed() {
        let mut bar = Bar::default();
        let first = bar.layout(&[
            Entry::Open,
            session("run:a", "claude/sample-project/a", true),
            Entry::Quit,
        ]);
        bar.began();
        assert!(bar.ended());
        // The menu closed with a pending change; two rebuilds may run before the click arrives.
        bar.layout(&[
            Entry::Open,
            session("run:b", "claude/sample-project/b", true),
            Entry::Quit,
        ]);
        let last = bar.layout(&[
            Entry::Quit,
            session("run:c", "claude/sample-project/c", true),
        ]);
        assert!(matches!(
            bar.action(id(&first[1])),
            Some(MenuAction::Session { key, .. }) if key == "run:a"
        ));
        assert_eq!(
            bar.action(id(&first[2])),
            Some(MenuAction::Quit),
            "never another entry's action"
        );
        assert_ne!(id(&first[0]), id(&last[0]), "a rebuilt menu has new ids");
        bar.began();
        assert_eq!(
            bar.action(id(&first[1])),
            None,
            "a menu shown since replaces the old one"
        );
    }
}
