//! The client's local preferences (Settings, phase 3b; see
//! `docs/superpowers/plans/2026-10-28-tauri-client-phase3b-settings.md`).
//!
//! They live in `<data_dir>/settings.json` under Qt's QSettings names, with
//! Qt's defaults and ranges (tray/src/SettingsPage.h and the headers it
//! reads). Under the switch identity the store is seeded once, read-only,
//! from Qt's own store; Qt's store is never written, so a rollback finds Qt as
//! it was. Loading, seeding and writing run on the store's own thread.

pub mod qt;

use qt::QtValue;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How a setting's value is read, as Qt reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// `toBool()`.
    Flag(bool),
    /// `toString()`: one of the choices, otherwise the first (Qt's
    /// `findData` falling back to index 0, or `== "ru" ? "ru" : "en"`).
    Choice(&'static [&'static str]),
    /// `toDouble()` (0 when it fails), then `qBound(min, value, max)`.
    Real { default: f64, min: f64, max: f64 },
    /// `toInt(&ok)`: the default when it fails, otherwise `qBound`.
    Whole { default: i64, min: i64, max: i64 },
    /// `toLongLong()` (0 when it fails), at least `min`.
    Long { default: i64, min: i64 },
    /// `toString()`.
    Text(&'static str),
    /// `toStringList()`: the shortcut group, one list of portable chord
    /// strings per command id, an empty list meaning no chord.
    Chords,
}

/// One setting: its QSettings name, how Qt reads it, and whether the
/// switch seeds it from Qt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub name: &'static str,
    pub kind: Kind,
    pub seeded: bool,
}

const fn key(name: &'static str, kind: Kind) -> Key {
    Key {
        name,
        kind,
        seeded: true,
    }
}

/// A setting of the client's own that Qt does not have: its `client/`
/// prefix keeps it apart from any key Qt adds later, and it is never seeded.
const fn client(name: &'static str, kind: Kind) -> Key {
    Key {
        name,
        kind,
        seeded: false,
    }
}

/// The prefix of the client's own settings.
pub const CLIENT: &str = "client/";

/// The group of shortcut overrides (`shortcuts/<command id>`, Shortcuts.h).
pub const SHORTCUTS: &str = "shortcuts/";

/// Qt's value of `automaticApprovals/enabled` at the switch, kept only so the
/// page can say Qt had it on. The client never enables automatic approvals
/// from it: the opt-in stays explicit (zerus-5wmh.54).
pub const QT_AUTOMATIC_APPROVALS: &str = "automaticApprovals/qtEnabledAtSwitch";

/// Every local setting the client has. Sources are in the plan's table.
pub const KEYS: &[Key] = &[
    // Appearance (SettingsPage.h:46-95, UiLanguage.h, ContentScale.h,
    // ActivityWidth.h, WindowLayer.cpp).
    key(
        "workspace/theme",
        Kind::Choice(&["system", "dark", "light"]),
    ),
    key("workspace/language", Kind::Choice(&["en", "ru"])),
    key(
        "workspace/usageIndicator",
        Kind::Choice(&["rails", "marker", "marker-countdown"]),
    ),
    key(
        "workspace/contentScale",
        Kind::Real {
            default: 1.0,
            min: 0.75,
            max: 2.0,
        },
    ),
    key(
        "workspace/activityWidth",
        Kind::Whole {
            default: 900,
            min: 480,
            max: 2400,
        },
    ),
    key("workspace/activityFullWidth", Kind::Flag(false)),
    key("workspace/alwaysOnTop", Kind::Flag(false)),
    // Sessions (SettingsPage.h:97-107, QuestionNotificationSettings.h).
    key("workspace/compact", Kind::Flag(false)),
    key("workspace/expandSessionsOnHover", Kind::Flag(true)),
    key("workspace/hideEmptyProjects", Kind::Flag(true)),
    key("workspace/animateActivity", Kind::Flag(true)),
    key("workspace/showCompletedSubagents", Kind::Flag(false)),
    key("workspace/reduceMotion", Kind::Flag(false)),
    key("notifications/optionalQuestions", Kind::Flag(false)),
    // Processes (ProcessSettings.h), web client phase 4 Task 11.
    key("processes/enabled", Kind::Flag(false)),
    key(
        "processes/activeSeconds",
        Kind::Real {
            default: 2.5,
            min: 1.0,
            max: 60.0,
        },
    ),
    key(
        "processes/backgroundSeconds",
        Kind::Real {
            default: 30.0,
            min: 5.0,
            max: 600.0,
        },
    ),
    key(
        "processes/unknownSeconds",
        Kind::Real {
            default: 10.0,
            min: 1.0,
            max: 600.0,
        },
    ),
    key(
        "processes/loadingSeconds",
        Kind::Real {
            default: 0.5,
            min: 0.0,
            max: 5.0,
        },
    ),
    // Voice input (speech/SpeechSettings.cpp, speech/SpeechComposer.cpp).
    // speech/microphone is not seeded: Qt keeps a QAudioDevice id, which
    // names no device of the client's.
    key("speech/preview_before_send", Kind::Flag(false)),
    key("speech/video_mode", Kind::Flag(false)),
    key("speech/manage_host", Kind::Text("")),
    key("speech/machine_id", Kind::Text("")),
    key("speech/profile", Kind::Text("")),
    key("speech/profile_revision", Kind::Long { default: 0, min: 0 }),
    Key {
        name: "speech/microphone",
        kind: Kind::Text(""),
        seeded: false,
    },
    // Files (DownloadCache), Updates (UpdateController.cpp).
    key("downloads/folder", Kind::Text("")),
    key("downloads/cacheBytes", Kind::Long { default: 0, min: 0 }),
    key("updates/channel", Kind::Text("stable")),
    key("updates/automatic", Kind::Flag(true)),
    // Automatic approvals: Qt's choice, for the notice only.
    key(QT_AUTOMATIC_APPROVALS, Kind::Flag(false)),
    // The client's own: its skin and the Overview column (design round 7).
    client("client/skin", Kind::Text("zerus-dark")),
    client(
        "client/overviewWidth",
        Kind::Whole {
            default: 1200,
            min: 720,
            max: 2400,
        },
    ),
    client("client/overviewFullWidth", Kind::Flag(false)),
];

/// The setting called `name`: a listed key, or a shortcut override.
pub fn find(name: &str) -> Option<Key> {
    KEYS.iter()
        .copied()
        .find(|key| key.name == name)
        .or_else(|| {
            let id = name.strip_prefix(SHORTCUTS)?;
            (!id.is_empty() && !id.contains('/')).then_some(Key {
                name: SHORTCUTS,
                kind: Kind::Chords,
                seeded: true,
            })
        })
}

/// A setting's value when nothing is stored.
pub fn default_value(kind: Kind) -> Value {
    match kind {
        Kind::Flag(on) => Value::Bool(on),
        Kind::Choice(choices) => Value::from(choices[0]),
        Kind::Real { default, .. } => json!(default),
        Kind::Whole { default, .. } | Kind::Long { default, .. } => Value::from(default),
        Kind::Text(text) => Value::from(text),
        Kind::Chords => Value::Array(Vec::new()),
    }
}

/// `value` read as Qt reads the setting, or its default.
pub fn effective(kind: Kind, value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return default_value(kind);
    };
    match kind {
        Kind::Flag(on) => Value::Bool(value.as_bool().unwrap_or(on)),
        Kind::Choice(choices) => Value::from(
            value
                .as_str()
                .filter(|text| choices.contains(text))
                .unwrap_or(choices[0]),
        ),
        Kind::Real { default, min, max } => {
            let number = value.as_f64().filter(|n| !n.is_nan()).unwrap_or(default);
            json!(number.clamp(min, max))
        }
        Kind::Whole { default, min, max } => {
            Value::from(value.as_i64().map_or(default, |n| n.clamp(min, max)))
        }
        Kind::Long { default, min } => Value::from(value.as_i64().map_or(default, |n| n.max(min))),
        Kind::Text(text) => Value::from(value.as_str().unwrap_or(text)),
        Kind::Chords => Value::Array(
            value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(Value::from)
                        .collect()
                })
                .unwrap_or_default(),
        ),
    }
}

/// Qt's stored value converted the way Qt's reader converts it.
pub fn from_qt(kind: Kind, value: &QtValue) -> Value {
    let stored = match kind {
        Kind::Flag(_) => Value::Bool(value.to_bool()),
        Kind::Choice(_) | Kind::Text(_) => Value::from(value.to_text()),
        Kind::Real { .. } => {
            let (number, _) = value.to_double();
            json!(if number.is_nan() { 0.0 } else { number })
        }
        // toInt(&ok) converts through a long long and keeps its low 32 bits.
        Kind::Whole { default, .. } => match value.to_long() {
            (number, true) => Value::from(i64::from(number as i32)),
            _ => Value::from(default),
        },
        Kind::Long { .. } => Value::from(value.to_long().0),
        Kind::Chords => Value::Array(value.to_text_list().into_iter().map(Value::from).collect()),
    };
    effective(kind, Some(&stored))
}

/// Where Qt keeps its settings: the format, read-only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QtStore {
    /// macOS: Qt's preference domain, read through cfprefsd (`defaults
    /// export`), which may hold values its plist file does not have yet;
    /// the file only when that fails.
    Plist {
        domain: Option<String>,
        path: PathBuf,
    },
    /// Linux `hgs-tray.conf` and Windows `hgs-tray.ini`.
    Ini(PathBuf),
}

/// macOS's preference tool, by its full path.
const DEFAULTS: &str = "/usr/bin/defaults";

/// What a read of Qt's store found, and from where: `qt-defaults`
/// (cfprefsd), `qt-plist` (the plist file) or `qt-ini`.
pub type QtValues = (BTreeMap<String, QtValue>, &'static str);

impl QtStore {
    /// Qt's store on this computer (tray/src/main.cpp:178, :276-277).
    pub fn here() -> Option<QtStore> {
        let home = || {
            std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(PathBuf::from)
        };
        if cfg!(windows) {
            std::env::var_os("APPDATA")
                .filter(|path| !path.is_empty())
                .map(|path| QtStore::Ini(PathBuf::from(path).join("hgdev").join("hgs-tray.ini")))
        } else if cfg!(target_os = "macos") {
            home().map(|home| QtStore::Plist {
                domain: Some("com.hgdev.hgs-tray".into()),
                path: home.join("Library/Preferences/com.hgdev.hgs-tray.plist"),
            })
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .or_else(|| home().map(|home| home.join(".config")))
                .map(|config| QtStore::Ini(config.join("hgdev").join("hgs-tray.conf")))
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            QtStore::Plist { path, .. } | QtStore::Ini(path) => path,
        }
    }

    /// Every value of the store by Qt name and where it was read;
    /// `Ok(None)` when the store does not exist.
    pub fn read(&self) -> Result<Option<QtValues>, String> {
        if let QtStore::Plist {
            domain: Some(domain),
            ..
        } = self
        {
            if let Some(values) = export_domain(domain) {
                return Ok(Some((values, "qt-defaults")));
            }
        }
        let bytes = match read_bounded(self.path(), MAX_QT_STORE) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        match self {
            QtStore::Plist { .. } => {
                qt::read_plist(&bytes).map(|values| Some((values, "qt-plist")))
            }
            QtStore::Ini(_) => Ok(Some((qt::read_ini(&bytes), "qt-ini"))),
        }
    }
}

/// `defaults export DOMAIN -`: the domain as cfprefsd holds it, read-only,
/// or `None` when the tool is missing, fails or prints no dictionary.
fn export_domain(domain: &str) -> Option<BTreeMap<String, QtValue>> {
    let invocation = crate::actions::Invocation {
        args: vec!["export".into(), domain.into(), "-".into()],
        stdin: None,
        timeout: Duration::from_secs(10),
        request_id: None,
    };
    match crate::run::run_limited(Path::new(DEFAULTS), &invocation, &[], MAX_QT_STORE as usize) {
        Ok(crate::actions::Exit::Code {
            code: 0, stdout, ..
        }) => qt::read_plist(&stdout).ok(),
        _ => None,
    }
}

const MAX_STORE: u64 = 4 * 1024 * 1024;
const MAX_QT_STORE: u64 = 16 * 1024 * 1024;

fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other("too large"));
    }
    Ok(bytes)
}

/// The values to seed from Qt's `values` for the settings named in `names`:
/// only those Qt has stored. `speech/manage_host` falls back to Qt's older
/// `speech/host` (SpeechSettings.cpp:233), and Qt's automatic approvals
/// choice goes to [`QT_AUTOMATIC_APPROVALS`].
pub fn seed_values(values: &BTreeMap<String, QtValue>, names: &[&str]) -> BTreeMap<String, Value> {
    let mut seeded = BTreeMap::new();
    for name in names {
        let Some(key) = find(name).filter(|key| key.seeded) else {
            continue;
        };
        let source = match *name {
            "speech/manage_host" => values
                .get("speech/manage_host")
                .or_else(|| values.get("speech/host")),
            QT_AUTOMATIC_APPROVALS => values.get("automaticApprovals/enabled"),
            other => values.get(other),
        };
        if let Some(value) = source {
            seeded.insert((*name).to_owned(), from_qt(key.kind, value));
        }
    }
    seeded
}

/// The names a seed considers: every seeded key plus Qt's shortcut overrides.
fn seed_names(values: &BTreeMap<String, QtValue>) -> Vec<String> {
    KEYS.iter()
        .filter(|key| key.seeded)
        .map(|key| key.name.to_owned())
        .chain(
            values
                .keys()
                .filter(|name| find(name).is_some_and(|key| key.kind == Kind::Chords))
                .cloned(),
        )
        .collect()
}

/// What `settings.json` holds.
#[derive(Clone, Debug, Default, PartialEq)]
struct Contents {
    values: Map<String, Value>,
    /// `{"source", "path", "at", "keys"}` once seeded from Qt.
    seed: Option<Value>,
}

impl Contents {
    fn parse(bytes: &[u8]) -> Option<Contents> {
        let Value::Object(mut object) = serde_json::from_slice(bytes).ok()? else {
            return None;
        };
        let values = match object.remove("values") {
            Some(Value::Object(values)) => values,
            _ => Map::new(),
        };
        let seed = object.remove("seed").filter(Value::is_object);
        Some(Contents { values, seed })
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut object = Map::new();
        object.insert("version".into(), Value::from(1));
        if let Some(seed) = &self.seed {
            object.insert("seed".into(), seed.clone());
        }
        object.insert("values".into(), Value::Object(self.values.clone()));
        serde_json::to_vec_pretty(&Value::Object(object)).unwrap_or_default()
    }
}

/// A change other code reacts to: the setting's name and its new value.
pub type Listener = Box<dyn Fn(&str, &Value) + Send + Sync>;

struct State {
    contents: Contents,
    loaded: bool,
    /// The store could not be read: settings run at their defaults and
    /// nothing is written over the unreadable file.
    unreadable: Option<String>,
    dirty: bool,
    writing: bool,
    closed: bool,
    seed: Option<QtStore>,
    error: Option<String>,
}

struct Shared {
    path: PathBuf,
    state: Mutex<State>,
    signal: Condvar,
    listeners: Mutex<Vec<Listener>>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The settings store: reads answer from memory once loaded, and writes go
/// to disk on the store's thread, the latest values only.
#[derive(Clone)]
pub struct Settings {
    shared: Arc<Shared>,
}

impl Settings {
    /// Opens `<data_dir>/settings.json`. With `seed`, a store never seeded
    /// before takes Qt's values first (the switch identity only).
    pub fn open(path: PathBuf, seed: Option<QtStore>) -> Settings {
        let shared = Arc::new(Shared {
            path,
            state: Mutex::new(State {
                contents: Contents::default(),
                loaded: false,
                unreadable: None,
                dirty: false,
                writing: false,
                closed: false,
                seed,
                error: None,
            }),
            signal: Condvar::new(),
            listeners: Mutex::new(Vec::new()),
        });
        let worker = Arc::clone(&shared);
        let started = std::thread::Builder::new()
            .name("settings".into())
            .spawn(move || run(&worker));
        if started.is_err() {
            let mut state = shared.lock();
            state.loaded = true;
            state.unreadable = Some("The settings thread did not start.".into());
            shared.signal.notify_all();
        }
        Settings { shared }
    }

    /// Waits until the store is loaded (and seeded, when asked).
    fn loaded(&self) -> std::sync::MutexGuard<'_, State> {
        let state = self.shared.lock();
        self.shared
            .signal
            .wait_while(state, |state| !state.loaded)
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The setting's value as Qt would read it, or `Null` for an unknown
    /// name.
    pub fn get(&self, name: &str) -> Value {
        let Some(key) = find(name) else {
            return Value::Null;
        };
        let state = self.loaded();
        effective(key.kind, state.contents.values.get(name))
    }

    pub fn flag(&self, name: &str) -> bool {
        self.get(name).as_bool().unwrap_or(false)
    }

    pub fn number(&self, name: &str) -> f64 {
        self.get(name).as_f64().unwrap_or(0.0)
    }

    pub fn text(&self, name: &str) -> String {
        self.get(name).as_str().unwrap_or_default().to_owned()
    }

    /// Every listed setting and every shortcut override, as Qt reads them.
    pub fn all(&self) -> Map<String, Value> {
        let state = self.loaded();
        let mut all: Map<String, Value> = KEYS
            .iter()
            .map(|key| {
                (
                    key.name.to_owned(),
                    effective(key.kind, state.contents.values.get(key.name)),
                )
            })
            .collect();
        for (name, value) in &state.contents.values {
            if name.starts_with(SHORTCUTS) && find(name).is_some() {
                all.insert(name.clone(), effective(Kind::Chords, Some(value)));
            }
        }
        all
    }

    /// Stores a value, normalized as Qt reads it, and returns what is now in
    /// effect. Removing a shortcut override (`Null`) restores its defaults.
    pub fn set(&self, name: &str, value: Value) -> Result<Value, String> {
        let key = find(name).ok_or_else(|| format!("Unknown setting: {name}"))?;
        if name == QT_AUTOMATIC_APPROVALS {
            return Err("This setting only records Qt's choice.".into());
        }
        let stored = match (&key.kind, &value) {
            (Kind::Chords, Value::Null) => None,
            (Kind::Flag(_), Value::Bool(_))
            | (Kind::Choice(_) | Kind::Text(_), Value::String(_))
            | (Kind::Chords, Value::Array(_)) => Some(effective(key.kind, Some(&value))),
            (Kind::Real { .. }, Value::Number(_)) => Some(effective(key.kind, Some(&value))),
            (Kind::Whole { .. } | Kind::Long { .. }, Value::Number(number))
                if number.is_i64() || number.is_u64() =>
            {
                Some(effective(key.kind, Some(&value)))
            }
            _ => return Err(format!("Wrong type for {name}")),
        };
        let now = {
            let mut state = self.loaded();
            if let Some(error) = &state.unreadable {
                return Err(error.clone());
            }
            match &stored {
                Some(value) => {
                    state.contents.values.insert(name.to_owned(), value.clone());
                }
                None => {
                    state.contents.values.remove(name);
                }
            }
            state.dirty = true;
            self.shared.signal.notify_all();
            effective(key.kind, stored.as_ref())
        };
        let listeners = self
            .shared
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for listener in listeners.iter() {
            listener(name, &now);
        }
        Ok(now)
    }

    /// Calls `listener` after every change made through [`Settings::set`].
    pub fn subscribe(&self, listener: Listener) {
        self.shared
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(listener);
    }

    /// The seed record (`source`, `path`, `at`, `keys`), once seeded.
    pub fn seed_record(&self) -> Option<Value> {
        self.loaded().contents.seed.clone()
    }

    /// Why the store could not be read or written, if it could not.
    pub fn error(&self) -> Option<String> {
        let state = self.loaded();
        state.unreadable.clone().or_else(|| state.error.clone())
    }

    /// Waits at most `limit` for every change to reach the disk (exit);
    /// whether it did.
    pub fn flush_within(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut state = self.shared.lock();
        loop {
            if state.loaded && !state.dirty && !state.writing {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .shared
                .signal
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        // The last handle besides the thread's: let the thread write and end.
        if Arc::strong_count(&self.shared) == 2 {
            self.shared.lock().closed = true;
            self.shared.signal.notify_all();
        }
    }
}

/// The store's thread: load, seed once when asked, then write the latest
/// values whenever they change.
fn run(shared: &Shared) {
    let (contents, unreadable) = match read_bounded(&shared.path, MAX_STORE) {
        Ok(bytes) => match Contents::parse(&bytes) {
            Some(contents) => (contents, None),
            None => (
                Contents::default(),
                Some(
                    "Settings are not readable; Zerus uses the defaults and keeps the file.".into(),
                ),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Contents::default(), None),
        Err(error) => (
            Contents::default(),
            Some(format!("Settings could not be read: {error}")),
        ),
    };
    let seed = shared.lock().seed.take();
    let mut contents = contents;
    let mut dirty = false;
    if let (Some(store), None) = (seed, &unreadable) {
        if contents.seed.is_none() {
            match seed_from(&store) {
                Ok((values, record)) => {
                    for (name, value) in values {
                        contents.values.entry(name).or_insert(value);
                    }
                    contents.seed = Some(record);
                    dirty = true;
                }
                Err(error) => eprintln!(
                    "zerus-client-core: settings seed: {error}; retried at the next start"
                ),
            }
        }
    }
    {
        let mut state = shared.lock();
        state.contents = contents;
        state.unreadable = unreadable;
        state.dirty = dirty;
        state.loaded = true;
        shared.signal.notify_all();
    }
    loop {
        let snapshot = {
            let state = shared.lock();
            let mut state = shared
                .signal
                .wait_while(state, |state| !state.dirty && !state.closed)
                .unwrap_or_else(PoisonError::into_inner);
            if !state.dirty {
                return;
            }
            state.dirty = false;
            state.writing = true;
            state.contents.to_bytes()
        };
        let written = write_store(&shared.path, &snapshot);
        let mut state = shared.lock();
        state.writing = false;
        state.error = written.err();
        shared.signal.notify_all();
        if state.closed && !state.dirty {
            return;
        }
    }
}

/// Qt's values for the seeded settings, and the record of the seed.
/// `Err` when Qt's store exists but could not be read: nothing is recorded,
/// so the next start tries again.
fn seed_from(store: &QtStore) -> Result<(BTreeMap<String, Value>, Value), String> {
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (values, source) = match store.read()? {
        Some((values, source)) => {
            let names = seed_names(&values);
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            (seed_values(&values, &names), source)
        }
        None => (BTreeMap::new(), "none"),
    };
    let domain = match store {
        QtStore::Plist { domain, .. } => domain.clone(),
        QtStore::Ini(_) => None,
    };
    let record = json!({
        "source": source,
        "domain": domain,
        "path": store.path().to_string_lossy(),
        "at": at,
        "keys": values.keys().collect::<Vec<_>>(),
    });
    Ok((values, record))
}

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Writes the store through a synced private temporary file and a rename,
/// so a cut-off write keeps the old file.
fn write_store(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_file_name(format!(
        ".settings.json.{}-{}.tmp",
        std::process::id(),
        TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = File::create(&temporary)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    written.map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("Settings not saved: {error}")
    })
}
