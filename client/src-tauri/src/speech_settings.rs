//! Settings > Voice input's speech providers: the core's state machine
//! (`zerus_client_core::speech_settings`) behind `speech_input`. Its calls
//! run on their own threads, and the view goes out as `speech-settings`
//! after each answer. A change's identity reaches the disk before the
//! change is sent; an editor's API key reaches only the call's input.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use zerus_client_core::default_hgs;
use zerus_client_core::speech_settings::{
    self, Call, Defaults, EditorForm, Effect, Platform, Providers, PENDING_KEY,
};

use crate::commands::AppState;
use crate::settings::Store;

/// The page's state, made when the page first asks.
#[derive(Default)]
pub struct SpeechPage(Arc<Inner>);

#[derive(Default)]
struct Inner {
    providers: Mutex<Option<Providers>>,
    /// The computers whose `hgs --help` named speech (and management).
    supported: Mutex<HashSet<String>>,
}

impl Inner {
    fn providers(&self) -> MutexGuard<'_, Option<Providers>> {
        self.providers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn supported(&self) -> MutexGuard<'_, HashSet<String>> {
        self.supported
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

fn platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::Macos
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

fn defaults(store: &Store) -> Defaults {
    Defaults {
        machine_id: store.0.text("speech/machine_id"),
        profile: store.0.text("speech/profile"),
        host: store.0.text("speech/host"),
    }
}

/// An action on the page; one per user action, so its size does not
/// matter. Tickets name what the core holds (the open editor, a removal
/// being confirmed), so the webview names no computer, revision or request
/// of its own.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum SpeechInput {
    /// The view as it is.
    Show,
    Refresh,
    ChooseHost {
        host: String,
    },
    Select {
        index: usize,
    },
    UseDefault,
    OpenEditor {
        create: bool,
    },
    CloseEditor {
        ticket: u64,
    },
    Save {
        ticket: u64,
        form: EditorForm,
    },
    AskRemove,
    Remove {
        ticket: u64,
        confirmed: bool,
    },
    Test,
    Check,
}

/// Applies an action and answers `{view, ask, media}`: the page, the remove
/// confirmation to show (for `ask_remove`), and whether this build lists
/// microphones.
#[tauri::command(async)]
pub fn speech_input(
    app: AppHandle,
    page: State<'_, SpeechPage>,
    input: SpeechInput,
) -> Result<Value, String> {
    let store = app.state::<Store>();
    let peers = app.state::<AppState>().peers();
    let mut ask = None;
    let (later, view) = {
        let mut guard = page.0.providers();
        let providers = guard.get_or_insert_with(|| {
            Providers::new(
                platform(),
                &store.0.text("speech/manage_host"),
                defaults(&store),
                &store.0.text(PENDING_KEY),
                || uuid::Uuid::new_v4().to_string(),
            )
        });
        if let Some(peers) = peers {
            providers.set_peers(peers.into_iter().collect());
        }
        providers.set_defaults(defaults(&store));
        let now = Instant::now();
        let effects = match input {
            SpeechInput::Show => Vec::new(),
            SpeechInput::Refresh => providers.refresh(),
            SpeechInput::ChooseHost { host } => providers.choose_host(&host),
            SpeechInput::Select { index } => {
                providers.select(index);
                Vec::new()
            }
            SpeechInput::UseDefault => providers.use_default(),
            SpeechInput::OpenEditor { create } => {
                providers.open_editor(create);
                Vec::new()
            }
            SpeechInput::CloseEditor { ticket } => {
                providers.close_editor(ticket);
                Vec::new()
            }
            SpeechInput::Save { ticket, form } => providers.save(ticket, form),
            SpeechInput::AskRemove => {
                ask = providers.ask_remove();
                Vec::new()
            }
            SpeechInput::Remove { ticket, confirmed } => providers.remove(ticket, confirmed),
            SpeechInput::Test => providers.test(),
            SpeechInput::Check => providers.check(now),
        };
        let later = settle(&store, providers, effects);
        (later, providers.view())
    };
    spawn(&app, &page.0, later);
    Ok(json!({ "view": view, "ask": ask, "media": cfg!(feature = "media") }))
}

/// Opens the provider documentation, at the section for `adapter`; the
/// text to show when no browser opens (SpeechSettings.cpp:55-59).
#[tauri::command]
pub fn open_speech_documentation(adapter: String) -> Result<(), String> {
    let url = speech_settings::documentation_url(&adapter);
    crate::settings::open_url(&url).map_err(|_| speech_settings::documentation_failure(&url))
}

/// What runs off the page's lock: a call, the wait for the change's record
/// to reach the disk, or the poll timer.
enum Later {
    Call(Call),
    Flush,
    Poll(String),
}

/// Applies the store effects under the page's lock, so they are ordered
/// with the state that asked for them (a record cleared before the next one
/// is kept, a default written before it is read back), and returns what
/// runs off the lock. A record the store refuses fails the change unsent.
fn settle(store: &Store, providers: &mut Providers, effects: Vec<Effect>) -> Vec<Later> {
    let mut later = Vec::new();
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        match effect {
            Effect::Set(values) => {
                for (key, value) in values {
                    let _ = store.0.set(key, value);
                }
            }
            Effect::ClearPending => speech_settings::clear_pending(&store.0),
            Effect::StorePending(text) => {
                if speech_settings::keep_pending(&store.0, &text) {
                    later.push(Later::Flush);
                } else {
                    queue.extend(providers.stored(false, Instant::now()));
                }
            }
            Effect::Call(call) => later.push(Later::Call(call)),
            Effect::PollLater(request) => later.push(Later::Poll(request)),
        }
    }
    later
}

/// Feeds the core one answer and settles its effects under the lock, then
/// sends the view and starts what runs off the lock.
fn answer(app: &AppHandle, inner: &Arc<Inner>, step: impl FnOnce(&mut Providers) -> Vec<Effect>) {
    let store = app.state::<Store>();
    let (later, view) = {
        let mut guard = inner.providers();
        let Some(providers) = guard.as_mut() else {
            return;
        };
        let effects = step(providers);
        let later = settle(&store, providers, effects);
        (later, providers.view())
    };
    let _ = app.emit("speech-settings", &view);
    spawn(app, inner, later);
}

fn spawn(app: &AppHandle, inner: &Arc<Inner>, later: Vec<Later>) {
    for work in later {
        let (app, inner) = (app.clone(), Arc::clone(inner));
        match work {
            Later::Call(Call { id, host, payload }) => {
                std::thread::spawn(move || {
                    let mut known = inner.supported().clone();
                    let reply = speech_settings::run(&default_hgs(), &host, &payload, &mut known);
                    // The payload may hold an API key: gone before anything else.
                    drop(payload);
                    inner.supported().extend(known);
                    answer(&app, &inner, |providers| {
                        providers.answered(id, reply, Instant::now())
                    });
                });
            }
            Later::Flush => {
                // The answer belongs to the one change being stored: the core
                // starts no other while one is stored or under way (busy and
                // pending refuse it), so no older flush can answer a newer one.
                std::thread::spawn(move || {
                    let written = speech_settings::pending_written(&app.state::<Store>().0);
                    answer(&app, &inner, |providers| {
                        providers.stored(written, Instant::now())
                    });
                });
            }
            Later::Poll(request) => {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(1));
                    answer(&app, &inner, |providers| {
                        providers.poll(&request, Instant::now())
                    });
                });
            }
        }
    }
}
