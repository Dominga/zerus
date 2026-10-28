//! One composer's voice driver: a [`VoiceComposer`] on its own thread that
//! carries out its effects. Drafts go through S5's [`DraftStore`] (durable
//! saves), calls through [`run_call`] on their own threads, the poll timer
//! runs in the driver's loop, and the recorder is a [`Capture`]
//! (client/media's cpal capture in the app). What the webview sees leaves
//! through the sink as [`Out`]; recordings never do.
use crate::drafts::DraftStore;
use crate::voice_composer::{
    run_call, shown, Ask, Effect, Failure, Inserted, Preferences, Shown, Target, Video,
    VoiceComposer,
};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What the recorder reports, from any thread.
#[derive(Clone, Debug, PartialEq)]
pub enum RecorderEvent {
    Started,
    Elapsed(u32),
    /// The video recorder is finishing its file.
    Preparing,
    /// The input level, about 30 times a second.
    Level {
        rms: f32,
        peak: f32,
    },
    Recorded {
        pcm: Vec<u8>,
        video: Option<Video>,
    },
    Failed(Failure, String),
    VideoFailed(String),
}

/// The recorder the driver opens; one recording at a time.
pub trait Capture: Send {
    fn start(
        &mut self,
        video: bool,
        microphone: &str,
        events: Box<dyn Fn(RecorderEvent) + Send + Sync>,
    );
    fn stop(&mut self, video: bool);
    fn cancel(&mut self);
}

/// What the driver tells the webview.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Out {
    /// The controls changed.
    Shown {
        shown: Box<Shown>,
    },
    Level {
        rms: f32,
        peak: f32,
    },
    /// Ask this and answer [`Input::Confirm`] with its ticket.
    Ask {
        ask: Ask,
    },
    /// The conversation's accepted message, for Activity.
    Message {
        key: String,
        message: Value,
    },
    /// Insert the transcript at the draft position recorded when Send or
    /// Check was pressed, save the draft, and answer [`Input::Inserted`].
    Insert {
        token: u64,
        text: String,
        id: String,
    },
    /// Open the microphone recovery dialog.
    PermissionRecovery,
    /// Remember the provider (`speech/machine_id`, `speech/profile`,
    /// `speech/profile_revision`).
    RememberProfile {
        profile: Map<String, Value>,
    },
    /// Pause other messages' players.
    PauseMessages,
    /// Stop the preview's playback.
    StopPlayback,
}

/// What the webview asks; one per user action, so its size does not matter.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Input {
    Update {
        target: Target,
        input_visible: bool,
        preferences: Preferences,
    },
    Press,
    /// Stop and preview, Space or Esc; `forget_send` when the composer hid
    /// or the application lost focus.
    Stop {
        forget_send: bool,
    },
    Send,
    Discard,
    Refresh,
    ChooseProvider(usize),
    Confirm {
        ticket: u64,
        confirmed: bool,
    },
    Inserted {
        token: u64,
        inserted: Inserted,
    },
    /// Whether the preview plays (Play or Pause).
    Playing(bool),
}

enum Message {
    Input(Input),
    Recorder(RecorderEvent),
    Loaded(u64, crate::drafts::StoredDraft),
    Saved(u64, bool),
    Answered(u64, Result<Map<String, Value>, String>),
    Quit,
}

/// A composer's voice driver; dropping it cancels a recording under way.
pub struct VoiceDriver {
    inbox: mpsc::Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl VoiceDriver {
    /// Starts the driver. `ids` gives fresh lower-case UUIDs; `sink` is
    /// called on the driver's thread.
    pub fn start(
        hgs: PathBuf,
        drafts: Arc<DraftStore>,
        capture: Box<dyn Capture>,
        sink: Box<dyn Fn(Out) + Send>,
        ids: impl FnMut() -> String + Send + 'static,
    ) -> VoiceDriver {
        let (inbox, messages) = mpsc::channel();
        let feed = inbox.clone();
        let thread = std::thread::Builder::new()
            .name("voice-composer".into())
            .spawn(move || {
                Loop {
                    composer: VoiceComposer::new(ids),
                    hgs,
                    drafts,
                    capture,
                    sink,
                    feed,
                    supported: Arc::new(Mutex::new(HashSet::new())),
                    timers: Vec::new(),
                    asks: HashMap::new(),
                    playing: false,
                    last: None,
                }
                .run(messages)
            })
            .ok();
        VoiceDriver { inbox, thread }
    }

    pub fn input(&self, input: Input) {
        let _ = self.inbox.send(Message::Input(input));
    }
}

impl Drop for VoiceDriver {
    fn drop(&mut self) {
        let _ = self.inbox.send(Message::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Loop {
    composer: VoiceComposer,
    hgs: PathBuf,
    drafts: Arc<DraftStore>,
    capture: Box<dyn Capture>,
    sink: Box<dyn Fn(Out) + Send>,
    feed: mpsc::Sender<Message>,
    supported: Arc<Mutex<HashSet<String>>>,
    timers: Vec<(Instant, u64)>,
    /// Which question each ticket answers: true for the cold cache.
    asks: HashMap<u64, bool>,
    playing: bool,
    last: Option<Shown>,
}

impl Loop {
    fn run(mut self, messages: mpsc::Receiver<Message>) {
        loop {
            let wait = self
                .timers
                .iter()
                .map(|(due, _)| due.saturating_duration_since(Instant::now()))
                .min()
                .unwrap_or(Duration::from_secs(3600));
            let effects = match messages.recv_timeout(wait) {
                Ok(Message::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.capture.cancel();
                    return;
                }
                Ok(message) => self.handle(message),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let now = Instant::now();
                    let due: Vec<u64> = self
                        .timers
                        .iter()
                        .filter(|(at, _)| *at <= now)
                        .map(|(_, token)| *token)
                        .collect();
                    self.timers.retain(|(at, _)| *at > now);
                    due.into_iter()
                        .flat_map(|token| self.composer.poll(token))
                        .collect()
                }
            };
            self.perform(effects);
            let current = shown(&self.composer, self.playing);
            if self.last.as_ref() != Some(&current) {
                self.last = Some(current.clone());
                (self.sink)(Out::Shown {
                    shown: Box::new(current),
                });
            }
        }
    }

    fn handle(&mut self, message: Message) -> Vec<Effect> {
        let composer = &mut self.composer;
        match message {
            Message::Input(input) => match input {
                Input::Update {
                    target,
                    input_visible,
                    preferences,
                } => composer.update(target, input_visible, preferences),
                Input::Press => composer.press(),
                Input::Stop { forget_send } => composer.stop_recording(forget_send),
                Input::Send => composer.send(),
                Input::Discard => composer.discard(),
                Input::Refresh => composer.refresh(),
                Input::ChooseProvider(index) => composer.choose_provider(index),
                Input::Confirm { ticket, confirmed } => match self.asks.remove(&ticket) {
                    Some(true) => composer.send_confirmed(ticket, confirmed),
                    Some(false) => composer.discard_confirmed(ticket, confirmed),
                    None => Vec::new(),
                },
                Input::Inserted { token, inserted } => composer.inserted(token, inserted),
                Input::Playing(playing) => {
                    self.playing = playing;
                    Vec::new()
                }
            },
            Message::Recorder(event) => match event {
                RecorderEvent::Started => composer.started(),
                RecorderEvent::Elapsed(seconds) => {
                    composer.elapsed(seconds);
                    Vec::new()
                }
                RecorderEvent::Preparing => {
                    composer.preparing();
                    Vec::new()
                }
                RecorderEvent::Level { rms, peak } => {
                    (self.sink)(Out::Level { rms, peak });
                    Vec::new()
                }
                RecorderEvent::Recorded { pcm, video } => composer.recorded(pcm, video),
                RecorderEvent::Failed(failure, error) => composer.failed(failure, &error),
                RecorderEvent::VideoFailed(error) => composer.video_failed(&error),
            },
            Message::Loaded(token, draft) => composer.loaded(token, Some(draft)),
            Message::Saved(token, ok) => composer.saved(token, ok),
            Message::Answered(token, reply) => composer.answered(token, reply),
            Message::Quit => Vec::new(),
        }
    }

    fn perform(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Record { video, microphone } => {
                    let feed = Mutex::new(self.feed.clone());
                    self.capture.start(
                        video,
                        &microphone,
                        Box::new(move |event| {
                            if let Ok(feed) = feed.lock() {
                                let _ = feed.send(Message::Recorder(event));
                            }
                        }),
                    );
                }
                Effect::StopRecorder { video } => self.capture.stop(video),
                Effect::CancelRecorder => self.capture.cancel(),
                Effect::PauseMessages => (self.sink)(Out::PauseMessages),
                Effect::StopPlayback => (self.sink)(Out::StopPlayback),
                Effect::Load { token, key } => {
                    let feed = self.feed.clone();
                    self.drafts.load(&key, move |loaded| {
                        let _ = feed.send(Message::Loaded(token, loaded.draft));
                    });
                }
                Effect::Save { token, key, draft } => {
                    let feed = self.feed.clone();
                    self.drafts.save_durable(&key, draft, move |result| {
                        if token != 0 {
                            let _ = feed.send(Message::Saved(token, result.is_ok()));
                        }
                    });
                }
                Effect::Call { token, call } => {
                    let (feed, hgs, supported) = (
                        self.feed.clone(),
                        self.hgs.clone(),
                        Arc::clone(&self.supported),
                    );
                    let _ =
                        std::thread::Builder::new()
                            .name("voice-call".into())
                            .spawn(move || {
                                let mut known = supported
                                    .lock()
                                    .map(|known| known.clone())
                                    .unwrap_or_default();
                                let reply = run_call(&hgs, &call, &mut known);
                                if let Ok(mut shared) = supported.lock() {
                                    shared.extend(known);
                                }
                                if token != 0 {
                                    let _ = feed.send(Message::Answered(token, reply));
                                }
                            });
                }
                Effect::RememberProfile(profile) => (self.sink)(Out::RememberProfile { profile }),
                Effect::Message { key, message } => (self.sink)(Out::Message { key, message }),
                Effect::Insert { token, text, id } => (self.sink)(Out::Insert { token, text, id }),
                Effect::PollLater { token } => self
                    .timers
                    .push((Instant::now() + Duration::from_secs(1), token)),
                Effect::PermissionRecovery => (self.sink)(Out::PermissionRecovery),
                Effect::Ask(ask) => {
                    let (ticket, cold) = match &ask {
                        Ask::ColdCache { ticket, .. } => (*ticket, true),
                        Ask::Discard { ticket, .. } => (*ticket, false),
                    };
                    self.asks.insert(ticket, cold);
                    (self.sink)(Out::Ask { ask });
                }
            }
        }
    }
}
