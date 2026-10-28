//! Settings > Voice input's speech providers: the core's state machine
//! (`zerus_client_core::speech_settings`) behind `speech_input`. Its calls
//! run on their own threads, and the view goes out as `speech-settings`
//! after each answer. A change's identity reaches the disk before the
//! change is sent; an editor's API key reaches only the call's input.

use std::collections::HashSet;
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

/// How long a change's identity may take to reach the disk before the
/// change is given up unsent.
const STORE_PENDING: Duration = Duration::from_secs(2);

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
    let (effects, view) = {
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
        (effects, providers.view())
    };
    run_effects(&app, &page.0, effects);
    Ok(json!({ "view": view, "ask": ask, "media": cfg!(feature = "media") }))
}

/// Opens the provider documentation, at the section for `adapter`; the
/// text to show when no browser opens (SpeechSettings.cpp:55-59).
#[tauri::command]
pub fn open_speech_documentation(adapter: String) -> Result<(), String> {
    let url = speech_settings::documentation_url(&adapter);
    crate::settings::open_url(&url)
        .map_err(|_| format!("Could not open the browser. Open this link manually:\n{url}"))
}

/// Feeds the core one answer, sends the view, then runs what follows.
fn answer(app: &AppHandle, inner: &Arc<Inner>, step: impl FnOnce(&mut Providers) -> Vec<Effect>) {
    let (effects, view) = {
        let mut guard = inner.providers();
        let Some(providers) = guard.as_mut() else {
            return;
        };
        let effects = step(providers);
        (effects, providers.view())
    };
    let _ = app.emit("speech-settings", &view);
    run_effects(app, inner, effects);
}

fn run_effects(app: &AppHandle, inner: &Arc<Inner>, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::Call(Call { id, host, payload }) => {
                let (app, inner) = (app.clone(), Arc::clone(inner));
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
            Effect::StorePending(text) => {
                let (app, inner) = (app.clone(), Arc::clone(inner));
                std::thread::spawn(move || {
                    let store = app.state::<Store>();
                    let written = store.0.set(PENDING_KEY, json!(text)).is_ok()
                        && store.0.flush_within(STORE_PENDING);
                    answer(&app, &inner, |providers| {
                        providers.stored(written, Instant::now())
                    });
                });
            }
            Effect::ClearPending => {
                let _ = app.state::<Store>().0.set(PENDING_KEY, json!(""));
            }
            Effect::Set(values) => {
                let store = app.state::<Store>();
                for (key, value) in values {
                    let _ = store.0.set(key, value);
                }
            }
            Effect::PollLater(request) => {
                let (app, inner) = (app.clone(), Arc::clone(inner));
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
