//! Freedesktop attention transport, following LinuxAttentionNotifier.cpp.
//! Detection stays in the existing desk; this adapter owns only native IDs.
//! One coalesced wakeup drives bounded desired state on a worker. Withdraw
//! changes that state immediately, including while Notify waits for a reply.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::zvariant::Value;
use zerus_client_core::attention::Notice;

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
pub const FILE: &str = "linux-notifications.json";
const LIMIT: usize = 8192;
const FILE_LIMIT: u64 = 16 * 1024 * 1024;
type Sink = Arc<dyn Fn(Event) + Send + Sync>;
type QtSeed = Box<dyn FnOnce() -> Result<Option<Saved>, String> + Send>;

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Activated { token: String, activation: String },
    Failed { token: String, detail: String },
    Uncertain(String),
}

#[derive(Default, Serialize, Deserialize)]
pub struct Saved {
    version: u32,
    owner: String,
    tokens: BTreeMap<u32, String>,
    /// A reservation survives an unknown Notify result. It is never reposted
    /// automatically: a late native banner may already exist without its ID.
    #[serde(default)]
    uncertain: BTreeSet<String>,
}
impl Saved {
    fn valid(&self) -> bool {
        self.version == 1
            && self.tokens.len() + self.uncertain.len() <= LIMIT
            && self
                .tokens
                .iter()
                .all(|(id, token)| *id > 0 && !token.is_empty() && token.len() <= 16384)
            && self
                .uncertain
                .iter()
                .all(|token| !token.is_empty() && token.len() <= 16384)
    }
}
struct Pending {
    nonce: u64,
    notice: Notice,
    sending: bool,
}
#[derive(Default)]
struct State {
    saved: Saved,
    activation: BTreeMap<u32, String>,
    pending: BTreeMap<String, Pending>,
    closes: BTreeSet<(String, u32)>,
    withdrawn: BTreeSet<String>,
    nonce: u64,
    ready: bool,
    disabled: Option<String>,
}
struct Shared {
    state: Mutex<State>,
    path: PathBuf,
    sink: Sink,
    wake: mpsc::SyncSender<()>,
    stopped: AtomicBool,
    done: AtomicBool,
    connection: Mutex<Option<Connection>>,
}
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}
fn persist(path: &Path, saved: &Saved) -> Result<(), String> {
    let bytes = serde_json::to_vec(saved).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(
            "Notification identities exceed the storage limit; the original file was preserved"
                .into(),
        );
    }
    crate::desk::write_private(path, &bytes)
        .map_err(|e| format!("Could not save notification identities: {e}"))
}
fn load(path: &Path, qt: QtSeed) -> Result<Saved, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let saved = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => {
            if !file
                .metadata()
                .map_err(|error| error.to_string())?
                .is_file()
            {
                return Err("Notification identities must be a regular file; the original path was preserved".into());
            }
            let mut bytes = Vec::new();
            file.take(FILE_LIMIT + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > FILE_LIMIT {
                return Err("Notification identities exceed the storage limit".into());
            }
            serde_json::from_slice::<Saved>(&bytes)
                .map_err(|e| format!("Could not read notification identities: {e}"))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => qt()?.unwrap_or(Saved {
            version: 1,
            ..Saved::default()
        }),
        Err(error) => return Err(error.to_string()),
    };
    if !saved.valid() {
        return Err(
            "Unsupported or invalid notification identities; the original file was preserved"
                .into(),
        );
    }
    Ok(saved)
}
pub fn qt_values(
) -> Result<Option<BTreeMap<String, zerus_client_core::settings::qt::QtValue>>, String> {
    zerus_client_core::settings::QtStore::here()
        .map(|store| store.read().map(|read| read.map(|(values, _)| values)))
        .unwrap_or(Ok(None))
}
fn qt_seed() -> Result<Option<Saved>, String> {
    let Some(values) = qt_values()? else {
        return Ok(None);
    };
    let owner = values
        .get("attention/linuxOwner")
        .map(|v| v.to_text())
        .unwrap_or_default();
    if owner.is_empty() {
        return Ok(None);
    }
    let tokens = values
        .get("attention/linuxTokens")
        .map(|v| v.to_text())
        .unwrap_or_default();
    let tokens = if tokens.is_empty() {
        BTreeMap::new()
    } else {
        serde_json::from_str(&tokens)
            .map_err(|e| format!("Could not read Qt notification identities: {e}"))?
    };
    Ok(Some(Saved {
        version: 1,
        owner,
        tokens,
        uncertain: BTreeSet::new(),
    }))
}

pub struct Poster {
    shared: Arc<Shared>,
}
impl Poster {
    fn spawn(path: PathBuf, qt: QtSeed, address: Option<String>, sink: Sink) -> Self {
        let (wake, receiver) = mpsc::sync_channel(1);
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            path,
            sink,
            wake,
            stopped: AtomicBool::new(false),
            done: AtomicBool::new(false),
            connection: Mutex::new(None),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("zerus-linux-notifications".into())
            .spawn(move || {
                if let Err(error) = run(worker.clone(), receiver, qt, address.as_deref()) {
                    let tokens = {
                        let mut state = lock(&worker.state);
                        state.disabled = Some(error.clone());
                        std::mem::take(&mut state.pending)
                            .into_keys()
                            .collect::<Vec<_>>()
                    };
                    for token in tokens {
                        (worker.sink)(Event::Failed {
                            token,
                            detail: error.clone(),
                        });
                    }
                    (worker.sink)(Event::Uncertain(error));
                }
                worker.done.store(true, Ordering::Release);
            })
            .expect("notification worker starts");
        Self { shared }
    }
    pub fn deliver(&self, cleared: Vec<String>, raised: Vec<Notice>) {
        let mut state = lock(&self.shared.state);
        let mut changed = false;
        for token in cleared {
            state.pending.remove(&token);
            state.saved.uncertain.remove(&token);
            if !state.ready && state.withdrawn.len() < LIMIT {
                state.withdrawn.insert(token.clone());
            }
            let ids: Vec<_> = state
                .saved
                .tokens
                .iter()
                .filter_map(|(id, shown)| (shown == &token).then_some(*id))
                .collect();
            for id in ids {
                state.saved.tokens.remove(&id);
                state.activation.remove(&id);
                let owner = state.saved.owner.clone();
                state.closes.insert((owner, id));
            }
            changed = true;
        }
        for notice in raised {
            if state
                .saved
                .tokens
                .values()
                .any(|token| *token == notice.token)
                || state.saved.uncertain.contains(&notice.token)
                || state.pending.contains_key(&notice.token)
            {
                continue;
            }
            let error = state.disabled.clone().or_else(|| {
                (state.saved.tokens.len()
                    + state.pending.len()
                    + state.closes.len()
                    + state.saved.uncertain.len()
                    >= LIMIT)
                    .then(|| "Too many unresolved system notifications".to_owned())
            });
            if let Some(detail) = error {
                (self.shared.sink)(Event::Failed {
                    token: notice.token,
                    detail,
                });
                continue;
            }
            state.nonce += 1;
            let nonce = state.nonce;
            state.pending.insert(
                notice.token.clone(),
                Pending {
                    nonce,
                    notice,
                    sending: false,
                },
            );
        }
        if changed && state.ready {
            if let Err(error) = persist(&self.shared.path, &state.saved) {
                (self.shared.sink)(Event::Uncertain(error));
            }
        }
        drop(state);
        let _ = self.shared.wake.try_send(());
    }
    fn stop(&self) {
        self.shared.stopped.store(true, Ordering::Relaxed);
        if let Some(connection) = lock(&self.shared.connection).take() {
            let _ = connection.close();
        }
        let _ = self.shared.wake.try_send(());
    }
}
impl Drop for Poster {
    fn drop(&mut self) {
        self.stop();
    }
}

fn owner(connection: &Connection) -> Result<String, String> {
    let bus = zbus::blocking::fdo::DBusProxy::new(connection).map_err(|error| error.to_string())?;
    match bus.get_name_owner(SERVICE.try_into().unwrap()) {
        Ok(name) => Ok(name.to_string()),
        Err(zbus::fdo::Error::NameHasNoOwner(_)) => Ok(String::new()),
        Err(error) => Err(error.to_string()),
    }
}
fn refresh_owner(shared: &Shared, connection: &Connection) -> Result<String, String> {
    // Serialize the lookup and publication: an older concurrent lookup cannot
    // restore IDs from a daemon the owner-change worker already retired.
    let mut state = lock(&shared.state);
    let owner = owner(connection)?;
    if state.saved.owner == owner {
        return Ok(owner);
    }
    state.saved.owner = owner.clone();
    state.saved.tokens.clear();
    state.saved.uncertain.clear();
    state.activation.clear();
    if state.ready {
        persist(&shared.path, &state.saved)?;
    }
    Ok(owner)
}
fn close(connection: &Connection, owner: &str, id: u32) {
    if owner.is_empty() {
        return;
    }
    if let Ok(proxy) = Proxy::new(connection, owner, PATH, SERVICE) {
        let _ = proxy.call::<_, _, ()>("CloseNotification", &(id,));
    }
}
fn escaped(body: &str) -> String {
    body.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn run(
    shared: Arc<Shared>,
    receiver: mpsc::Receiver<()>,
    qt: QtSeed,
    address: Option<&str>,
) -> Result<(), String> {
    let builder = match address {
        Some(address) => zbus::blocking::connection::Builder::address(address),
        None => zbus::blocking::connection::Builder::session(),
    }
    .map_err(|e| e.to_string())?;
    let connection = builder
        .method_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    *lock(&shared.connection) = Some(connection.clone());
    if shared.stopped.load(Ordering::Relaxed) {
        let _ = connection.close();
        return Ok(());
    }
    // Subscribe before loading IDs/looking up the owner. Both iterators have
    // bounded native queues, and each signal is verified against its owner.
    let native_rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(SERVICE)
        .unwrap()
        .path(PATH)
        .unwrap()
        .build();
    let owner_rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")
        .unwrap()
        .interface("org.freedesktop.DBus")
        .unwrap()
        .member("NameOwnerChanged")
        .unwrap()
        .add_arg(SERVICE)
        .unwrap()
        .build();
    let signals = MessageIterator::for_match_rule(native_rule, &connection, Some(256))
        .map_err(|e| e.to_string())?;
    let owners = MessageIterator::for_match_rule(owner_rule, &connection, Some(16))
        .map_err(|e| e.to_string())?;
    {
        let mut state = lock(&shared.state);
        state.saved = load(&shared.path, qt)?;
        let withdrawn = std::mem::take(&mut state.withdrawn);
        for token in withdrawn {
            state.saved.uncertain.remove(&token);
            let ids: Vec<_> = state
                .saved
                .tokens
                .iter()
                .filter_map(|(id, shown)| (shown == &token).then_some(*id))
                .collect();
            for id in ids {
                state.saved.tokens.remove(&id);
                let old = state.saved.owner.clone();
                state.closes.insert((old, id));
            }
        }
        let current = owner(&connection)?;
        if state.saved.owner != current {
            state.saved.owner = current;
            state.saved.tokens.clear();
            state.saved.uncertain.clear();
        }
        state.ready = true;
        persist(&shared.path, &state.saved)?;
    }
    let events = shared.clone();
    let signals_worker = std::thread::spawn(move || {
        for message in signals {
            let Ok(message) = message else { break };
            signal(&events, &message);
        }
    });
    let ownership = shared.clone();
    let owner_bus = connection.clone();
    let owner_worker = std::thread::spawn(move || {
        for message in owners {
            let Ok(message) = message else { break };
            if message
                .body()
                .deserialize::<(String, String, String)>()
                .is_ok()
            {
                // A queued older owner-change must not overwrite a newer lookup.
                if let Err(error) = refresh_owner(&ownership, &owner_bus) {
                    (ownership.sink)(Event::Uncertain(error));
                }
            }
        }
    });
    loop {
        if shared.stopped.load(Ordering::Relaxed) {
            break;
        }
        let job = {
            let mut state = lock(&shared.state);
            if let Some((owner, id)) = state.closes.pop_first() {
                Some(Work::Close(owner, id))
            } else {
                let next = state
                    .pending
                    .iter()
                    .find(|(_, pending)| !pending.sending)
                    .map(|(token, _)| token.clone());
                next.map(|token| {
                    if state.saved.tokens.values().any(|shown| shown == &token)
                        || state.saved.uncertain.contains(&token)
                    {
                        state.pending.remove(&token);
                        return Work::Continue;
                    }
                    let pending = state.pending.get_mut(&token).unwrap();
                    pending.sending = true;
                    Work::Post(pending.nonce, pending.notice.clone())
                })
            }
        };
        match job {
            Some(Work::Close(owner, id)) => close(&connection, &owner, id),
            Some(Work::Post(nonce, notice)) => post(&shared, &connection, nonce, notice),
            Some(Work::Continue) => {}
            None => {
                if receiver.recv().is_err() {
                    break;
                }
            }
        }
    }
    let _ = connection.close();
    let _ = signals_worker.join();
    let _ = owner_worker.join();
    Ok(())
}
enum Work {
    Close(String, u32),
    Post(u64, Notice),
    Continue,
}
fn post(shared: &Shared, connection: &Connection, nonce: u64, notice: Notice) {
    let current = match refresh_owner(shared, connection) {
        Ok(current) => current,
        Err(detail) => {
            let mut state = lock(&shared.state);
            let wanted = state
                .pending
                .get(&notice.token)
                .is_some_and(|pending| pending.nonce == nonce);
            if wanted {
                state.pending.remove(&notice.token);
            }
            drop(state);
            if wanted {
                (shared.sink)(Event::Failed {
                    token: notice.token,
                    detail,
                });
            }
            return;
        }
    };
    {
        let mut state = lock(&shared.state);
        if !state
            .pending
            .get(&notice.token)
            .is_some_and(|pending| pending.nonce == nonce)
        {
            return;
        }
        if current.is_empty() {
            state.pending.remove(&notice.token);
            drop(state);
            (shared.sink)(Event::Failed {
                token: notice.token,
                detail: "System notification service is unavailable".into(),
            });
            return;
        }
        state.saved.uncertain.insert(notice.token.clone());
        if let Err(detail) = persist(&shared.path, &state.saved) {
            state.saved.uncertain.remove(&notice.token);
            state.pending.remove(&notice.token);
            drop(state);
            (shared.sink)(Event::Failed {
                token: notice.token,
                detail,
            });
            return;
        }
    }
    let result = (|| {
        let proxy = Proxy::new(connection, current.as_str(), PATH, SERVICE)?;
        let hints = BTreeMap::from([
            ("desktop-entry", Value::from("hgs-tray")),
            ("urgency", Value::from(1_u8)),
            ("category", Value::from("im.received")),
        ]);
        proxy.call::<_, _, u32>(
            "Notify",
            &(
                "Zerus",
                0_u32,
                "hgs-zerus-swarm-symbolic",
                notice.title.as_str(),
                escaped(&notice.body),
                vec!["default", "Open session", "open-session", "Open session"],
                hints,
                -1_i32,
            ),
        )
    })();
    let mut state = lock(&shared.state);
    let wanted = state
        .pending
        .get(&notice.token)
        .is_some_and(|pending| pending.nonce == nonce);
    if wanted {
        state.pending.remove(&notice.token);
    }
    match result {
        Ok(id) if id > 0 => {
            if wanted && state.saved.owner == current {
                state.saved.uncertain.remove(&notice.token);
                state.saved.tokens.insert(id, notice.token);
            } else {
                // Close only the daemon that returned this ID. Its replacement
                // may already have reused the integer for another application's card.
                drop(state);
                close(connection, &current, id);
                return;
            }
        }
        Err(zbus::Error::MethodError(name, detail, _))
            if wanted
                && !matches!(
                    name.as_str(),
                    "org.freedesktop.DBus.Error.NoReply"
                        | "org.freedesktop.DBus.Error.Timeout"
                        | "org.freedesktop.DBus.Error.TimedOut"
                        | "org.freedesktop.DBus.Error.Disconnected"
                ) =>
        {
            state.saved.uncertain.remove(&notice.token);
            (shared.sink)(Event::Failed {
                token: notice.token,
                detail: detail.unwrap_or_else(|| "System notification was refused".into()),
            });
        }
        Err(error) if wanted => (shared.sink)(Event::Uncertain(format!(
            "System notification delivery is unconfirmed: {error}"
        ))),
        Ok(_) if wanted => (shared.sink)(Event::Uncertain(
            "System notification delivery returned no identity".into(),
        )),
        _ => {}
    }
    if let Err(error) = persist(&shared.path, &state.saved) {
        (shared.sink)(Event::Uncertain(error));
    }
}
fn signal(shared: &Shared, message: &zbus::Message) {
    let header = message.header();
    let mut state = lock(&shared.state);
    if header
        .sender()
        .is_none_or(|sender| sender.as_str() != state.saved.owner)
    {
        return;
    }
    let mut event = None;
    match header.member().map(|member| member.as_str()) {
        Some("ActivationToken") => {
            if let Ok((id, token)) = message.body().deserialize::<(u32, String)>() {
                if state.saved.tokens.contains_key(&id) {
                    state.activation.insert(id, token);
                }
            }
        }
        Some("ActionInvoked") => {
            if let Ok((id, action)) = message.body().deserialize::<(u32, String)>() {
                if action == "default" || action == "open-session" {
                    if let Some(token) = state.saved.tokens.get(&id).cloned() {
                        let activation = state.activation.remove(&id).unwrap_or_default();
                        event = Some(Event::Activated { token, activation });
                    }
                }
            }
        }
        Some("NotificationClosed") => {
            if let Ok((id, reason)) = message.body().deserialize::<(u32, u32)>() {
                // Plasma's expired banner remains clickable in notification history.
                if reason != 1 {
                    state.saved.tokens.remove(&id);
                    state.activation.remove(&id);
                    if let Err(error) = persist(&shared.path, &state.saved) {
                        event = Some(Event::Uncertain(error));
                    }
                }
            }
        }
        _ => {}
    }
    drop(state);
    if let Some(event) = event {
        (shared.sink)(event);
    }
}

pub fn install(
    app: &AppHandle,
    dir: PathBuf,
    switch: bool,
    desk: mpsc::Sender<crate::desk::DeskInput>,
) {
    let target = app.clone();
    let sink = Arc::new(move |event| match event {
        Event::Activated { token, activation } => {
            if zerus_client_core::updates::is_update_token(&token) {
                if crate::updates::notification_current(&target, &token) {
                    let app = target.clone();
                    let _ = target.run_on_main_thread(move || {
                        crate::platform::linux_tray::present(&app, Some(&activation));
                        crate::updates::open_page(&app);
                    });
                }
            } else {
                let _ = desk.send(crate::desk::DeskInput::LinuxClicked { token, activation });
            }
        }
        Event::Failed { token, detail } => {
            if !zerus_client_core::updates::is_update_token(&token) {
                let _ = desk.send(crate::desk::DeskInput::Failed { token, detail });
            }
        }
        Event::Uncertain(detail) => crate::desk::window_notice(&target, "Zerus", &detail),
    });
    app.manage(Poster::spawn(
        dir.join(FILE),
        Box::new(move || if switch { qt_seed() } else { Ok(None) }),
        None,
        sink,
    ));
    // GTK's native file chooser can be active while the main Tauri window is
    // unfocused. Qt suppresses banners for the whole foreground application.
    let app = app.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(250), move || {
        use gtk::prelude::*;
        if app
            .try_state::<Poster>()
            .is_some_and(|poster| poster.shared.stopped.load(Ordering::Relaxed))
        {
            return gtk::glib::ControlFlow::Break;
        }
        crate::desk::set_focused(
            &app,
            gtk::Window::list_toplevels()
                .into_iter()
                .filter_map(|widget| widget.downcast::<gtk::Window>().ok())
                .any(|window| window.is_active()),
        );
        gtk::glib::ControlFlow::Continue
    });
}
pub fn deliver(app: &AppHandle, cleared: Vec<String>, raised: Vec<Notice>) {
    if let Some(poster) = app.try_state::<Poster>() {
        poster.deliver(cleared, raised);
    }
}
pub fn stop(app: &AppHandle) {
    if let Some(poster) = app.try_state::<Poster>() {
        poster.stop();
    }
}

#[cfg(test)]
#[path = "linux_notify_tests.rs"]
mod tests;
