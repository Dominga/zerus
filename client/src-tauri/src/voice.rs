//! Voice messages (the `media` feature): one voice driver per composer key,
//! holding the composer's state machine and its microphone capture. The
//! webview sends inputs and receives [`Out`] on its channel: the controls'
//! state, the level, questions, accepted messages and transcripts to
//! insert. Recordings stay in Rust.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use tauri::ipc::Channel;
use tauri::State;
use zerus_client_core::default_hgs;
use zerus_client_core::drafts::DraftStore;
use zerus_client_core::voice_composer::{Inserted, Preferences, Target};
use zerus_client_core::voice_driver::{Input, Out, VoiceDriver};
use zerus_client_media::{microphones, video_available, MediaCapture};

/// Voice drivers open at once; a composer closes its own when it goes.
pub const MAX_DRIVERS: usize = 32;
const TOO_MANY: &str = "Too many voice composers are open. Close one and try again.";
const REPLAY_NEEDS_HGS: &str = "Voice in replay mode needs ZERUS_CLIENT_HGS.";

/// The open voice drivers, by composer key.
pub struct Voices {
    hgs: PathBuf,
    drafts: Arc<DraftStore>,
    drivers: Mutex<HashMap<String, VoiceDriver>>,
}

impl Voices {
    /// `drafts` is the app's one draft store, shared with the composers.
    pub fn new(drafts: Arc<DraftStore>) -> Voices {
        Voices {
            hgs: default_hgs(),
            drafts,
            drivers: Mutex::new(HashMap::new()),
        }
    }

    /// At exit: closes every driver, cancelling recordings under way and
    /// waiting until their draft saves are queued. Blocks, bounded.
    pub fn close_all(&self) {
        let drivers: Vec<VoiceDriver> = self
            .drivers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .map(|(_, driver)| driver)
            .collect();
        for driver in drivers {
            driver.close();
        }
    }
}

/// An input from the webview; one per user action, so its size does not
/// matter.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum VoiceInput {
    Update {
        target: Target,
        input_visible: bool,
        preferences: Preferences,
    },
    Press,
    Stop {
        forget_send: bool,
    },
    Send,
    Discard,
    Refresh,
    ChooseProvider {
        index: usize,
    },
    Confirm {
        ticket: u64,
        confirmed: bool,
    },
    Inserted {
        token: u64,
        inserted: InsertedAs,
    },
    Playing {
        playing: bool,
    },
}

/// How a transcript's insertion ended.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertedAs {
    /// The draft changed; nothing was inserted.
    Refused,
    Saved,
    NotSaved,
}

impl From<VoiceInput> for Input {
    fn from(input: VoiceInput) -> Input {
        match input {
            VoiceInput::Update {
                target,
                input_visible,
                preferences,
            } => Input::Update {
                target,
                input_visible,
                preferences,
            },
            VoiceInput::Press => Input::Press,
            VoiceInput::Stop { forget_send } => Input::Stop { forget_send },
            VoiceInput::Send => Input::Send,
            VoiceInput::Discard => Input::Discard,
            VoiceInput::Refresh => Input::Refresh,
            VoiceInput::ChooseProvider { index } => Input::ChooseProvider(index),
            VoiceInput::Confirm { ticket, confirmed } => Input::Confirm { ticket, confirmed },
            VoiceInput::Inserted { token, inserted } => Input::Inserted {
                token,
                inserted: match inserted {
                    InsertedAs::Refused => Inserted::Refused,
                    InsertedAs::Saved => Inserted::Saved(true),
                    InsertedAs::NotSaved => Inserted::Saved(false),
                },
            },
            VoiceInput::Playing { playing } => Input::Playing(playing),
        }
    }
}

/// Opens the voice driver of a composer, replacing an earlier one for the
/// same key; its outputs go to `events`. A replay reaches only an explicit
/// `ZERUS_CLIENT_HGS`, never the real CLI.
#[tauri::command]
pub async fn voice_open(
    state: State<'_, Voices>,
    key: String,
    events: Channel<Out>,
) -> Result<(), String> {
    if crate::actions::blocked() {
        return Err(REPLAY_NEEDS_HGS.into());
    }
    {
        let drivers = state.drivers.lock().unwrap_or_else(PoisonError::into_inner);
        if !drivers.contains_key(&key) && drivers.len() >= MAX_DRIVERS {
            return Err(TOO_MANY.into());
        }
    }
    let driver = VoiceDriver::start(
        state.hgs.clone(),
        Arc::clone(&state.drafts),
        Box::new(MediaCapture::default()),
        Box::new(move |out| {
            let _ = events.send(out);
        }),
        || uuid::Uuid::new_v4().to_string(),
    );
    let previous = state
        .drivers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, driver);
    // Dropping never waits: the replaced driver's thread ends on its own.
    drop(previous);
    Ok(())
}

/// An input for an open voice driver.
#[tauri::command]
pub async fn voice_input(
    state: State<'_, Voices>,
    key: String,
    input: VoiceInput,
) -> Result<(), String> {
    let drivers = state.drivers.lock().unwrap_or_else(PoisonError::into_inner);
    let driver = drivers
        .get(&key)
        .ok_or_else(|| "The voice composer is not open.".to_owned())?;
    driver.input(input.into());
    Ok(())
}

/// Closes a composer's voice driver without waiting; a recording under way
/// is cancelled, a saved one stays in its draft.
#[tauri::command]
pub async fn voice_close(state: State<'_, Voices>, key: String) -> Result<(), String> {
    let driver = state
        .drivers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&key);
    drop(driver);
    Ok(())
}

/// A microphone Settings > Voice input offers.
#[derive(Clone, Debug, Serialize)]
pub struct MicrophoneRow {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// This computer's microphones; reading the devices can take a moment.
#[tauri::command]
pub async fn voice_microphones() -> Result<Vec<MicrophoneRow>, String> {
    let found = tauri::async_runtime::spawn_blocking(microphones)
        .await
        .map_err(|error| error.to_string())?;
    Ok(found
        .into_iter()
        .map(|microphone| MicrophoneRow {
            id: microphone.id,
            name: microphone.name,
            default: microphone.default,
        })
        .collect())
}

/// Whether this build records round video; the Video mode hides without.
#[tauri::command]
pub fn voice_video_available() -> bool {
    video_available()
}
