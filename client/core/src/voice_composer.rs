//! The voice composer, ported from the Qt client's `SpeechComposer`: record
//! a voice (or round video) message, keep it as a durable draft, choose the
//! speech provider, preview, send it once, and recover an interrupted or
//! refused send without ever sending the audio twice on its own. In a draft
//! composer (dictation) the recording is transcribed into the draft instead.
//!
//! The composer does no I/O and shows nothing. Each input returns
//! [`Effect`]s; the caller carries them out off the UI thread and answers
//! with the effect's token: [`Effect::Load`] and [`Effect::Save`] through
//! S5's draft store ([`crate::drafts`], durable saves),
//! [`Effect::Call`] with [`run_call`], [`Effect::Insert`] through the
//! composer's draft, and [`Effect::Ask`] with a confirmation. [`shown`]
//! gives what the controls show.
//!
//! What is written when: the recording as soon as it stops; the submit's
//! request ID (`submitted`) before the audio is sent; a refusal when it
//! arrives; and an empty draft once the message is accepted or discarded.
//! The draft key is `voice-message-draft\n{session key}`.
//!
//! Reference: `tray/src/speech/SpeechComposer.cpp`, `SpeechClient.cpp`.
use crate::actions::Exit;
use crate::attachments::{Attachment, Draft};
use crate::drafts::StoredDraft;
use crate::voice::{self, MAX_PCM_BYTES};
use crate::ComputerId;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// The shortest recording kept: 0.1 s.
pub const MIN_PCM_BYTES: usize = 3200;
/// Dictation polls its transcript this many times, a second apart.
pub const DRAFT_POLLS: u32 = 120;
/// The longest transcript taken.
pub const MAX_TRANSCRIPT_BYTES: usize = 100_000;

pub const INVALID_RECORDING: &str = "The voice recording is invalid.";
pub const INVALID_SAVED: &str = "The saved voice recording is invalid.";
pub const INTERRUPTED: &str =
    "Submission was interrupted. Check this message; audio is never sent twice automatically.";
pub const CONVERSATION_CHANGED: &str =
    "The conversation changed. The recording is kept without sending.";
pub const NO_PROVIDER: &str =
    "No speech provider is available. Refresh to try again; your recording is kept.";
pub const PROVIDER_CHANGED: &str = "The selected voice provider is unavailable or its configuration changed. Refresh or explicitly choose a provider; your recording is kept.";
pub const VIDEO_NEEDS_UPDATE: &str =
    "Update hgs on the conversation computer to send video messages.";
pub const REVIEW_PROVIDER: &str =
    "Review the transcription provider before sending. The recording is kept.";
pub const UNAVAILABLE_PROVIDER: &str = "Selected provider (unavailable)";
pub const SAVE_RECORDING_FAILED: &str =
    "Could not save this recording. Keep Zerus open and try Send again.";
pub const SAVE_SELECTION_FAILED: &str =
    "Could not save the voice provider selection. Keep Zerus open and try again.";
pub const TARGET_CHANGED: &str =
    "The conversation changed or cannot receive input. The recording is kept without sending.";
pub const SAVE_REQUEST_FAILED: &str = "Could not save the voice request. No audio was sent.";
pub const SAVE_TRANSCRIPTION_FAILED: &str =
    "Could not save the transcription request. No audio was sent.";
pub const REFUSED: &str =
    "The computer did not accept this recording. Send voice retries the same recording and provider.";
pub const SAVE_REFUSAL_FAILED: &str =
    "Could not save the refusal. Check this message before trying again.";
pub const UNCONFIRMED: &str =
    "Voice submission is not confirmed. Check this message before trying another recording.";
pub const VIDEO_UNCONFIRMED: &str =
    "Video acceptance is not confirmed. Check this message before trying again.";
pub const RECEIPT_NOT_SAVED: &str =
    "The message was accepted, but its local receipt could not be saved. Check it again.";
pub const DISCARD_FAILED: &str = "Could not discard the saved recording. Try again.";
pub const TRANSCRIPTION_UNCONFIRMED: &str =
    "Transcription is unconfirmed. Check the existing request.";
pub const TRANSCRIPTION_PENDING: &str =
    "Transcription is still pending. Check it later; the recording is kept.";
pub const NO_TRANSCRIPT: &str = "The recording has no confirmed transcript. The audio is kept.";
pub const SAVE_TRANSCRIPT_FAILED: &str = "Could not save the transcript. The recording is kept.";
pub const TRANSCRIPT_NOT_SAVED: &str =
    "The transcript could not be saved. Add to draft retries the same insertion.";
pub const RECEIPT_NOT_CLEARED: &str =
    "The transcript is in the draft. Its recording receipt could not be cleared.";
pub const DRAFT_CHANGED: &str = "The draft changed during transcription. Add to draft inserts the saved transcript when you choose.";
pub const DRAFT_NOTICE: &str =
    "Open the original conversation to review or check this voice message.";

/// The composer's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Idle,
    /// The recorder is opening.
    Starting,
    Recording,
    /// The recording, or a step's state, is being saved.
    Saving,
    /// A saved recording waits: play, choose a provider, send or discard.
    Preview,
    Submitting,
    /// A send's or transcription's outcome is unknown: Check, never resend.
    Uncertain,
}

/// What the notice chip opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    #[default]
    General,
    /// Microphone access was denied: the recovery dialog.
    MicrophonePermission,
}

/// The conversation the composer records for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    /// The composer's session key.
    pub key: String,
    pub host: String,
    pub session: String,
    pub run: String,
    pub conversation: String,
    /// The cold-cache warning before sending, empty for none.
    pub warning: String,
    pub available: bool,
    /// Why an unavailable target never takes voice: the button's tooltip.
    pub reason: String,
    /// A draft composer: dictation into the draft.
    pub draft_only: bool,
}

impl Target {
    pub fn same_conversation(&self, other: &Target) -> bool {
        self.draft_only == other.draft_only
            && self.key == other.key
            && self.host == other.host
            && self.session == other.session
            && self.run == other.run
            && self.conversation == other.conversation
    }
}

/// The user's voice settings when recording starts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    /// Record a round video instead of voice.
    pub video_mode: bool,
    /// Stop to preview instead of sending at once.
    pub preview_before_send: bool,
    /// The chosen microphone, empty for the default.
    pub microphone: String,
    /// The last provider used (`speech/machine_id`, `speech/profile`).
    pub machine_id: String,
    pub profile_id: String,
}

/// Why the recorder failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    PermissionDenied,
    Other,
}

/// A recorded round video.
#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    pub mp4: Vec<u8>,
    /// `duration_ms`, `width`, `height` and `poster_base64`.
    pub meta: Map<String, Value>,
}

/// A speech or voice call.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// `hgs [@host] voice SESSION --json`.
    Voice {
        host: String,
        session: String,
        payload: Value,
    },
    /// `hgs speech --json` on this computer.
    Speech { payload: Value },
}

/// A confirmation the view asks; its answer goes back with the ticket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ask {
    /// "Continue with a cold cache?" with the warning, "Send with full
    /// context" and Cancel (the default); answer [`VoiceComposer::send_confirmed`].
    ColdCache { warning: String, ticket: u64 },
    /// "Discard voice recording?", "Delete the saved, unsent voice recording
    /// for {session}?", "Discard recording" and Cancel (the default); answer
    /// [`VoiceComposer::discard_confirmed`].
    Discard { session: String, ticket: u64 },
}

/// What the caller does.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Open the recorder (round video with `video`); report started,
    /// elapsed seconds, the recording or a failure.
    Record {
        video: bool,
        microphone: String,
    },
    StopRecorder {
        video: bool,
    },
    CancelRecorder,
    /// Pause other messages' players before recording.
    PauseMessages,
    StopPlayback,
    /// Load the draft and answer [`VoiceComposer::loaded`].
    Load {
        token: u64,
        key: String,
    },
    /// Save the draft durably (an empty draft clears it) and answer
    /// [`VoiceComposer::saved`]; token 0 needs no answer.
    Save {
        token: u64,
        key: String,
        draft: StoredDraft,
    },
    /// Run the call ([`run_call`]) and answer [`VoiceComposer::answered`].
    Call {
        token: u64,
        call: Call,
    },
    /// Remember the provider chosen (`speech/machine_id`, `speech/profile`,
    /// `speech/profile_revision`).
    RememberProfile(Map<String, Value>),
    /// The conversation's accepted message, for its Activity.
    Message {
        key: String,
        message: Value,
    },
    /// Insert the transcript into the draft at the place recorded when Send
    /// or Check was pressed, then answer [`VoiceComposer::inserted`].
    Insert {
        token: u64,
        text: String,
        id: String,
    },
    /// Answer [`VoiceComposer::poll`] in one second.
    PollLater {
        token: u64,
    },
    /// Show the microphone recovery dialog ([`permission_text`]).
    PermissionRecovery,
    Ask(Ask),
}

/// How an insertion ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inserted {
    /// The draft changed: nothing was inserted.
    Refused,
    /// Inserted; whether the draft was saved.
    Saved(bool),
}

#[derive(Clone, Debug)]
enum Pending {
    Load {
        generation: u64,
    },
    Save {
        generation: u64,
        any_generation: bool,
        then: Then,
    },
    Profiles {
        generation: u64,
        request: u64,
        target: Target,
    },
    Voice {
        generation: u64,
        request: Map<String, Value>,
        target: Target,
        key: String,
    },
    Draft {
        generation: u64,
    },
    Poll {
        generation: u64,
    },
    Insert {
        generation: u64,
        id: String,
    },
}

#[derive(Clone, Debug)]
enum Then {
    Selection,
    Recorded {
        send_after: bool,
    },
    TranscriptionRequest,
    Send {
        call: Box<Call>,
        previous_attempt: i64,
        target: Target,
        key: String,
    },
    Refusal,
    Accepted,
    Discarded,
    Transcript,
    ReceiptCleared {
        id: String,
    },
}

/// A provider in the list: its label and its profile (empty for the
/// unavailable placeholder).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub label: String,
    pub profile: Map<String, Value>,
}

/// An exact saved recording, read by the native preview host only.
/// The native host sends a bounded raw copy only when Play is requested.
pub struct Preview {
    pub id: String,
    pub pcm: Vec<u8>,
    pub video: Vec<u8>,
}

/// One composer's voice recording.
pub struct VoiceComposer {
    state: State,
    notice: String,
    notice_kind: NoticeKind,
    profile_error: String,
    current: Target,
    input_visible: bool,
    target: Target,
    loaded_key: String,
    loading: bool,
    preferences: Preferences,
    pcm: Vec<u8>,
    transcript: String,
    video: Vec<u8>,
    video_meta: Map<String, Value>,
    capture_video: bool,
    id: String,
    operation_id: String,
    rejection: Map<String, Value>,
    attempted_rejection_generation: i64,
    selected_profile: Map<String, Value>,
    profile_bound: bool,
    providers: Vec<Provider>,
    current_provider: Option<usize>,
    profiles_loading: bool,
    profiles_validated: bool,
    video_supported: bool,
    send_after_saved: bool,
    preview_before_send: bool,
    seconds: u32,
    draft_polls: u32,
    generation: u64,
    profile_generation: u64,
    tokens: u64,
    pending: HashMap<u64, Pending>,
    asks: HashMap<u64, (u64, String, Target, Map<String, Value>)>,
    effects: Vec<Effect>,
    ids: Box<dyn FnMut() -> String + Send>,
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn integer(value: Option<&Value>) -> i64 {
    value
        .and_then(|number| {
            number
                .as_i64()
                .or_else(|| number.as_f64().map(|f| f as i64))
        })
        .unwrap_or(0)
}

/// JSON equality as Qt's `QJsonValue ==`: numbers by value.
pub fn json_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| json_equal(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, value)| y.get(key).is_some_and(|other| json_equal(value, other)))
        }
        _ => a == b,
    }
}

fn same_field(a: &Map<String, Value>, b: &Map<String, Value>, key: &str) -> bool {
    match (a.get(key), b.get(key)) {
        (None, None) => true,
        (Some(x), Some(y)) => json_equal(x, y),
        _ => false,
    }
}

fn canonical_uuid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(at, c)| {
            if matches!(at, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_digit() || ('a'..='f').contains(&c)
            }
        })
        && text.chars().any(|c| c != '-' && c != '0')
}

fn profile_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn digest(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|text| {
        text.len() == 64
            && text
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    })
}

impl VoiceComposer {
    /// A composer; `ids` gives fresh lower-case UUIDs.
    pub fn new(ids: impl FnMut() -> String + Send + 'static) -> VoiceComposer {
        VoiceComposer {
            state: State::Idle,
            notice: String::new(),
            notice_kind: NoticeKind::General,
            profile_error: String::new(),
            current: Target::default(),
            input_visible: false,
            target: Target::default(),
            loaded_key: String::new(),
            loading: false,
            preferences: Preferences::default(),
            pcm: Vec::new(),
            transcript: String::new(),
            video: Vec::new(),
            video_meta: Map::new(),
            capture_video: false,
            id: String::new(),
            operation_id: String::new(),
            rejection: Map::new(),
            attempted_rejection_generation: 0,
            selected_profile: Map::new(),
            profile_bound: false,
            providers: Vec::new(),
            current_provider: None,
            profiles_loading: false,
            profiles_validated: false,
            video_supported: false,
            send_after_saved: false,
            preview_before_send: false,
            seconds: 0,
            draft_polls: 0,
            generation: 0,
            profile_generation: 0,
            tokens: 0,
            pending: HashMap::new(),
            asks: HashMap::new(),
            effects: Vec::new(),
            ids: Box::new(ids),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// The recording's message ID, empty without one.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// A Play request can only read this exact saved recording, never live capture.
    pub fn preview(&self, id: &str) -> Option<Preview> {
        (matches!(self.state, State::Preview | State::Uncertain)
            && !id.is_empty()
            && self.id == id
            && !self.pcm.is_empty())
        .then(|| Preview {
            id: self.id.clone(),
            pcm: self.pcm.clone(),
            video: self.video.clone(),
        })
    }

    /// The draft store key of the loaded session.
    pub fn storage_key(&self) -> String {
        format!("voice-message-draft\n{}", self.loaded_key)
    }

    fn token(&mut self, pending: Pending) -> u64 {
        self.tokens += 1;
        self.pending.insert(self.tokens, pending);
        self.tokens
    }

    fn take(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    fn preferred(&self) -> Map<String, Value> {
        let mut profile = Map::new();
        if !self.preferences.machine_id.is_empty() || !self.preferences.profile_id.is_empty() {
            profile.insert("machine_id".into(), json!(self.preferences.machine_id));
            profile.insert("profile_id".into(), json!(self.preferences.profile_id));
        }
        profile
    }

    fn reset_profiles(&mut self) {
        self.profiles_loading = false;
        self.profiles_validated = false;
        self.profile_error.clear();
        self.selected_profile = self.preferred();
        self.profile_bound = false;
        self.providers.clear();
        self.current_provider = None;
    }

    fn clear_recording(&mut self) {
        self.pcm.clear();
        self.transcript.clear();
        self.video.clear();
        self.video_meta.clear();
        self.id.clear();
        self.rejection.clear();
        self.attempted_rejection_generation = 0;
        self.operation_id.clear();
    }

    /// The composer's conversation as it is now, with whether its input is
    /// visible; also when its controls change. A new session key drops the
    /// recording under way and loads that session's voice draft.
    pub fn update(
        &mut self,
        current: Target,
        input_visible: bool,
        preferences: Preferences,
    ) -> Vec<Effect> {
        self.current = current;
        self.input_visible = input_visible;
        self.preferences = preferences;
        self.sync();
        self.take()
    }

    fn sync(&mut self) {
        let key = self.current.key.clone();
        if key == self.loaded_key {
            return;
        }
        self.send_after_saved = false;
        self.generation += 1;
        self.effects.push(Effect::CancelRecorder);
        self.effects.push(Effect::StopPlayback);
        self.loaded_key = key.clone();
        self.target = self.current.clone();
        self.clear_recording();
        self.capture_video = false;
        self.notice.clear();
        self.notice_kind = NoticeKind::General;
        self.state = State::Idle;
        self.loading = !key.is_empty();
        self.reset_profiles();
        if key.is_empty() {
            return;
        }
        let token = self.token(Pending::Load {
            generation: self.generation,
        });
        self.effects.push(Effect::Load {
            token,
            key: self.storage_key(),
        });
    }

    /// The voice draft as loaded (None without one).
    pub fn loaded(&mut self, token: u64, draft: Option<StoredDraft>) -> Vec<Effect> {
        let Some(Pending::Load { generation }) = self.pending.remove(&token) else {
            return Vec::new();
        };
        if generation != self.generation {
            return Vec::new();
        }
        self.loading = false;
        let Some(draft) = draft.filter(|draft| !draft.draft.attachments.is_empty()) else {
            return self.take();
        };
        let meta = Value::Object(draft.form_state.clone());
        if let Some(speech) = meta
            .get("speech")
            .and_then(Value::as_object)
            .filter(|speech| !speech.is_empty())
        {
            self.selected_profile = speech.clone();
            self.profile_bound = true;
        }
        self.target = Target {
            key: self.loaded_key.clone(),
            host: text(&meta, "host").into(),
            session: text(&meta, "session").into(),
            run: text(&meta, "run").into(),
            conversation: text(&meta, "conversation").into(),
            draft_only: meta
                .get("draft_only")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            ..Target::default()
        };
        text(&meta, "transcript").clone_into(&mut self.transcript);
        let audio = &draft.draft.attachments[0].bytes;
        if !audio.starts_with(b"RIFF")
            || audio.get(8..16) != Some(b"WAVEfmt ".as_slice())
            || audio.get(36..40) != Some(b"data".as_slice())
            || audio.len() < 3244
        {
            self.notice = INVALID_SAVED.into();
            return self.take();
        }
        self.video_meta = meta
            .get("video")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if !self.video_meta.is_empty() && draft.draft.attachments.len() == 2 {
            self.video = draft.draft.attachments[1].bytes.to_vec();
            self.capture_video = true;
        }
        self.pcm = audio[44..].to_vec();
        text(&meta, "id").clone_into(&mut self.id);
        self.seconds = (self.pcm.len() / 32000) as u32;
        self.operation_id = meta
            .get("operation_id")
            .and_then(Value::as_str)
            .unwrap_or(&self.id)
            .to_owned();
        self.rejection = meta
            .get("rejection")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        self.attempted_rejection_generation = integer(meta.get("attempted_rejection_generation"));
        let submitted = meta
            .get("submitted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.state = if submitted {
            State::Uncertain
        } else {
            State::Preview
        };
        if submitted {
            self.notice = INTERRUPTED.into();
        }
        self.load_profiles();
        self.take()
    }

    fn identity(&mut self, action: &str) -> Map<String, Value> {
        let request = (self.ids)();
        voice::identity(
            action,
            &request,
            &self.target.run,
            &self.target.conversation,
        )
    }

    fn load_profiles(&mut self) {
        if self.profiles_loading {
            return;
        }
        self.profile_generation += 1;
        self.profiles_loading = true;
        self.profiles_validated = false;
        self.profile_error.clear();
        let target = self.target.clone();
        let call = if target.draft_only {
            Call::Speech {
                payload: json!({"action": "draft_profiles"}),
            }
        } else {
            Call::Voice {
                host: target.host.clone(),
                session: target.session.clone(),
                payload: Value::Object(self.identity("profiles")),
            }
        };
        let token = self.token(Pending::Profiles {
            generation: self.generation,
            request: self.profile_generation,
            target,
        });
        self.effects.push(Effect::Call { token, call });
    }

    /// Refresh: reads the providers again, keeping the recording.
    pub fn refresh(&mut self) -> Vec<Effect> {
        self.load_profiles();
        self.take()
    }

    fn profiles(&mut self, reply: Result<Map<String, Value>, String>, target: &Target) {
        self.profiles_loading = false;
        if !self.current.same_conversation(target) || !self.target.same_conversation(target) {
            self.profile_error = CONVERSATION_CHANGED.into();
            return;
        }
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                self.profile_error = error;
                return;
            }
        };
        let preferred_machine = self
            .selected_profile
            .get("machine_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let preferred_id = self
            .selected_profile
            .get("profile_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let (mut selected, mut profile_match, mut profile_matches) = (None, None, 0);
        self.providers.clear();
        self.current_provider = None;
        for computer in reply
            .get("computers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let machine = text(computer, "machine_id");
            for profile in computer
                .get("profiles")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if profile.get("available").and_then(Value::as_bool) != Some(true)
                    || !canonical_uuid(machine)
                {
                    continue;
                }
                let id = text(profile, "id");
                if !profile_id(id) {
                    continue;
                }
                let mut data = Map::new();
                data.insert("machine_id".into(), json!(machine));
                data.insert("profile_id".into(), json!(id));
                if let Some(revision) = profile.get("profile_revision") {
                    let Some(number) = revision.as_f64() else {
                        continue;
                    };
                    if !(1.0..=9_007_199_254_740_991.0).contains(&number)
                        || number.floor() != number
                    {
                        continue;
                    }
                    data.insert("profile_revision".into(), revision.clone());
                }
                let name = profile.get("name").and_then(Value::as_str).unwrap_or(id);
                let label = format!("{} / {name}", text(computer, "name"));
                let index = self.providers.len();
                let revision_fits = !self.profile_bound
                    || same_field(&data, &self.selected_profile, "profile_revision");
                if machine == preferred_machine && id == preferred_id && revision_fits {
                    selected = Some(index);
                }
                if preferred_machine.is_empty() && revision_fits && id == preferred_id {
                    profile_match = Some(index);
                    profile_matches += 1;
                }
                self.providers.push(Provider {
                    label,
                    profile: data,
                });
            }
        }
        if selected.is_none() && profile_matches == 1 {
            selected = profile_match;
        }
        if selected.is_none() && self.selected_profile.is_empty() && !self.providers.is_empty() {
            selected = Some(0);
        }
        if let Some(index) = selected {
            self.current_provider = Some(index);
            self.selected_profile = self.providers[index].profile.clone();
            self.profile_bound = true;
        } else if !self.selected_profile.is_empty() {
            self.providers.push(Provider {
                label: UNAVAILABLE_PROVIDER.into(),
                profile: Map::new(),
            });
            self.current_provider = Some(self.providers.len() - 1);
        }
        self.video_supported = reply
            .get("features")
            .and_then(Value::as_array)
            .is_some_and(|features| features.iter().any(|feature| feature == "voice_video"));
        self.profiles_validated = true;
        if selected.is_none() {
            self.profile_error = if self.selected_profile.is_empty() {
                NO_PROVIDER
            } else {
                PROVIDER_CHANGED
            }
            .into();
        }
        if self.capture_video && !self.video_supported {
            self.profile_error = VIDEO_NEEDS_UPDATE.into();
        }
        self.save_profile_selection();
    }

    /// The provider whose profile a send uses, empty unless the list is
    /// validated and the chosen row is the bound profile.
    pub fn selected_profile(&self) -> Map<String, Value> {
        let chosen = self
            .current_provider
            .and_then(|index| self.providers.get(index))
            .map(|provider| provider.profile.clone())
            .unwrap_or_default();
        let named = |key: &str| {
            chosen
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty())
        };
        if self.profiles_validated
            && !self.profiles_loading
            && json_equal(
                &Value::Object(chosen.clone()),
                &Value::Object(self.selected_profile.clone()),
            )
            && named("machine_id")
            && named("profile_id")
        {
            chosen
        } else {
            Map::new()
        }
    }

    /// The user chose a provider row. Not while a refusal binds the
    /// recording to its provider.
    pub fn choose_provider(&mut self, index: usize) -> Vec<Effect> {
        if !self.rejection.is_empty() || index >= self.providers.len() {
            return Vec::new();
        }
        self.current_provider = Some(index);
        let profile = self.providers[index].profile.clone();
        if !profile.is_empty() {
            self.selected_profile = profile.clone();
            self.profile_bound = true;
            self.profile_error.clear();
            self.effects.push(Effect::RememberProfile(profile));
            self.save_profile_selection();
        }
        self.take()
    }

    fn save_profile_selection(&mut self) {
        if self.pcm.is_empty() || !matches!(self.state, State::Preview | State::Saving) {
            return;
        }
        self.save(Then::Selection, false);
    }

    fn draft(&self) -> StoredDraft {
        let mut form = Map::new();
        form.insert("kind".into(), json!("voice_message"));
        form.insert("id".into(), json!(self.id));
        form.insert("host".into(), json!(self.target.host));
        form.insert("session".into(), json!(self.target.session));
        form.insert("run".into(), json!(self.target.run));
        form.insert("conversation".into(), json!(self.target.conversation));
        form.insert(
            "speech".into(),
            Value::Object(self.selected_profile.clone()),
        );
        form.insert("draft_only".into(), json!(self.target.draft_only));
        form.insert("transcript".into(), json!(self.transcript));
        form.insert(
            "submitted".into(),
            json!(matches!(self.state, State::Submitting | State::Uncertain)),
        );
        form.insert("rejection".into(), Value::Object(self.rejection.clone()));
        form.insert(
            "attempted_rejection_generation".into(),
            json!(self.attempted_rejection_generation),
        );
        form.insert("operation_id".into(), json!(self.operation_id));
        let attachment = |name: &str, mime: &str, bytes: Vec<u8>| Attachment {
            name: name.into(),
            mime: mime.into(),
            bytes: Arc::from(bytes),
            reference: String::new(),
        };
        let mut attachments = vec![attachment("voice.wav", "audio/wav", voice::wav(&self.pcm))];
        if !self.video.is_empty() {
            attachments.push(attachment("video.mp4", "video/mp4", self.video.clone()));
            form.insert("video".into(), Value::Object(self.video_meta.clone()));
        }
        StoredDraft {
            draft: Draft {
                attachments,
                ..Draft::default()
            },
            label: format!("Voice message ({})", self.target.session),
            form_state: form,
            ..StoredDraft::default()
        }
    }

    /// A durable save of the recording; `any_generation` answers even after
    /// the composer moved on (an explicit Send owns its target).
    fn save(&mut self, then: Then, any_generation: bool) {
        let draft = self.draft();
        let token = self.token(Pending::Save {
            generation: self.generation,
            any_generation,
            then,
        });
        self.effects.push(Effect::Save {
            token,
            key: self.storage_key(),
            draft,
        });
    }

    fn clear(&mut self, then: Option<Then>) {
        let token = match then {
            Some(then) => self.token(Pending::Save {
                generation: self.generation,
                any_generation: false,
                then,
            }),
            None => 0,
        };
        self.effects.push(Effect::Save {
            token,
            key: self.storage_key(),
            draft: StoredDraft::default(),
        });
    }

    /// Voice (or Video) pressed: starts recording, or while recording sends
    /// it (or stops to preview).
    pub fn press(&mut self) -> Vec<Effect> {
        if self.state == State::Recording {
            self.request_send_recording();
        } else {
            self.start();
        }
        self.take()
    }

    fn start(&mut self) {
        self.sync();
        let target = self.current.clone();
        if self.loading
            || self.state != State::Idle
            || !target.available
            || (!target.draft_only && (target.run.is_empty() || target.conversation.is_empty()))
            || !self.input_visible
        {
            return;
        }
        self.effects.push(Effect::PauseMessages);
        self.video.clear();
        self.video_meta.clear();
        self.capture_video = !target.draft_only && self.preferences.video_mode;
        self.send_after_saved = false;
        self.preview_before_send = target.draft_only || self.preferences.preview_before_send;
        self.target = target;
        self.transcript.clear();
        self.notice.clear();
        self.notice_kind = NoticeKind::General;
        self.state = State::Starting;
        self.generation += 1;
        self.reset_profiles();
        self.effects.push(Effect::Record {
            video: self.capture_video,
            microphone: self.preferences.microphone.clone(),
        });
        self.load_profiles();
    }

    fn request_send_recording(&mut self) {
        if self.state != State::Recording {
            return;
        }
        if self.preview_before_send {
            self.send_after_saved = false;
            self.stop();
            return;
        }
        // A Send click never waits for late provider discovery to become a
        // future Send.
        self.send_after_saved =
            !self.selected_profile().is_empty() && (!self.capture_video || self.video_supported);
        if !self.send_after_saved {
            self.notice = if self.capture_video && !self.video_supported {
                VIDEO_NEEDS_UPDATE
            } else {
                REVIEW_PROVIDER
            }
            .into();
        }
        self.stop();
    }

    /// Stop and preview, Space or Esc, or the composer hid or the
    /// application lost focus (which also forgets a pending Send).
    pub fn stop_recording(&mut self, forget_send: bool) -> Vec<Effect> {
        if forget_send {
            self.send_after_saved = false;
            self.effects.push(Effect::StopPlayback);
        }
        self.stop();
        self.take()
    }

    fn stop(&mut self) {
        if self.state == State::Starting {
            self.generation += 1;
            self.effects.push(Effect::CancelRecorder);
            self.send_after_saved = false;
            self.state = State::Idle;
            return;
        }
        if self.state != State::Recording {
            return;
        }
        self.state = State::Saving;
        self.effects.push(Effect::StopRecorder {
            video: self.capture_video,
        });
    }

    /// The recorder opened.
    pub fn started(&mut self) -> Vec<Effect> {
        if self.state == State::Starting {
            self.state = State::Recording;
            self.seconds = 0;
        }
        self.take()
    }

    /// Seconds recorded so far.
    pub fn elapsed(&mut self, seconds: u32) {
        self.seconds = seconds;
    }

    /// The video recorder is finishing its file.
    pub fn preparing(&mut self) {
        if self.capture_video && self.state == State::Recording {
            self.state = State::Saving;
        }
    }

    /// The recording (16 kHz mono PCM) and, for a video, its file.
    pub fn recorded(&mut self, pcm: Vec<u8>, video: Option<Video>) -> Vec<Effect> {
        if !matches!(self.state, State::Saving | State::Recording) {
            return Vec::new();
        }
        if let Some(video) = video {
            self.video = video.mp4;
            self.video_meta = video.meta;
        }
        if pcm.len() < MIN_PCM_BYTES || pcm.len() > MAX_PCM_BYTES {
            self.fail(INVALID_RECORDING, NoticeKind::General);
            return self.take();
        }
        self.pcm = pcm;
        self.seconds = (self.pcm.len() / 32000) as u32;
        self.id = (self.ids)();
        self.state = State::Saving;
        let send_after = std::mem::take(&mut self.send_after_saved);
        self.save(Then::Recorded { send_after }, false);
        self.take()
    }

    /// The recorder failed; a denied microphone opens its recovery dialog
    /// while the composer is visible.
    pub fn failed(&mut self, failure: Failure, error: &str) -> Vec<Effect> {
        if !matches!(
            self.state,
            State::Starting | State::Recording | State::Saving
        ) {
            return Vec::new();
        }
        let denied = failure == Failure::PermissionDenied;
        self.fail(
            error,
            if denied {
                NoticeKind::MicrophonePermission
            } else {
                NoticeKind::General
            },
        );
        if denied && self.input_visible {
            self.effects.push(Effect::PermissionRecovery);
        }
        self.take()
    }

    /// A round video recording failed.
    pub fn video_failed(&mut self, error: &str) -> Vec<Effect> {
        if self.capture_video {
            self.fail(error, NoticeKind::General);
        }
        self.take()
    }

    fn fail(&mut self, error: &str, kind: NoticeKind) {
        self.send_after_saved = false;
        self.state = State::Idle;
        error.clone_into(&mut self.notice);
        self.notice_kind = kind;
    }

    /// Send voice (or Transcribe / Add to draft): sends the saved recording
    /// once; a cold cache asks first.
    pub fn send(&mut self) -> Vec<Effect> {
        if self.state == State::Uncertain {
            self.check();
        } else {
            self.send_now();
        }
        self.take()
    }

    fn send_now(&mut self) {
        if self.target.draft_only {
            if self.state != State::Preview {
                return;
            }
            if !self.transcript.is_empty() {
                self.apply_transcript();
                return;
            }
            if self.selected_profile().is_empty()
                || !self.current.same_conversation(&self.target)
                || !self.current.available
            {
                return;
            }
            self.draft_polls = 0;
            self.state = State::Submitting;
            self.notice.clear();
            self.save(Then::TranscriptionRequest, false);
            return;
        }
        let chosen = self.selected_profile();
        if self.state != State::Preview || self.pcm.is_empty() || chosen.is_empty() {
            return;
        }
        if !self.video.is_empty() && !self.video_supported {
            self.notice = VIDEO_NEEDS_UPDATE.into();
            return;
        }
        if !self.current.available || !self.current.same_conversation(&self.target) {
            self.notice = TARGET_CHANGED.into();
            return;
        }
        if !self.current.warning.is_empty() {
            self.tokens += 1;
            let ticket = self.tokens;
            self.asks.insert(
                ticket,
                (
                    self.generation,
                    self.id.clone(),
                    self.target.clone(),
                    chosen,
                ),
            );
            self.effects.push(Effect::Ask(Ask::ColdCache {
                warning: self.current.warning.clone(),
                ticket,
            }));
            return;
        }
        self.submit(chosen);
    }

    /// The cold-cache question's answer: sends only if nothing changed while
    /// it was open.
    pub fn send_confirmed(&mut self, ticket: u64, confirmed: bool) -> Vec<Effect> {
        let Some((generation, id, target, chosen)) = self.asks.remove(&ticket) else {
            return Vec::new();
        };
        if !confirmed
            || generation != self.generation
            || id != self.id
            || self.state != State::Preview
            || !self.target.same_conversation(&target)
            || !self.current.available
            || !self.current.same_conversation(&target)
            || self.selected_profile() != chosen
        {
            return self.take();
        }
        self.submit(chosen);
        self.take()
    }

    fn submit(&mut self, chosen: Map<String, Value>) {
        self.effects.push(Effect::StopPlayback);
        self.notice.clear();
        self.state = State::Submitting;
        let mut payload = self.identity("submit");
        payload.insert("request_id".into(), json!(self.id));
        payload.insert("speech".into(), Value::Object(chosen.clone()));
        let previous_attempt = self.attempted_rejection_generation;
        if !self.rejection.is_empty() {
            payload.insert("request_id".into(), json!((self.ids)()));
            payload.insert("message_id".into(), json!(self.id));
            payload.insert("rejection".into(), Value::Object(self.rejection.clone()));
            self.attempted_rejection_generation = integer(self.rejection.get("generation"));
        }
        self.operation_id = text(&Value::Object(payload.clone()), "request_id").to_owned();
        self.effects.push(Effect::RememberProfile(chosen));
        payload.insert("audio".into(), voice::audio(&self.pcm));
        if !self.video.is_empty() {
            let mut media = self.video_meta.clone();
            media.insert(
                "data_base64".into(),
                json!(base64::engine::general_purpose::STANDARD.encode(&self.video)),
            );
            payload.insert("video".into(), Value::Object(media));
        }
        let call = Call::Voice {
            host: self.target.host.clone(),
            session: self.target.session.clone(),
            payload: Value::Object(payload),
        };
        let (target, key) = (self.target.clone(), self.storage_key());
        self.save(
            Then::Send {
                call: Box::new(call),
                previous_attempt,
                target,
                key,
            },
            true,
        );
    }

    fn check(&mut self) {
        if self.target.draft_only {
            if self.state == State::Uncertain {
                self.draft_polls = 0;
                self.state = State::Submitting;
                self.draft_request(false);
            }
            return;
        }
        if self.id.is_empty() || self.state != State::Uncertain {
            return;
        }
        self.state = State::Submitting;
        let mut payload = self.identity("status");
        payload.insert("message_id".into(), json!(self.id));
        self.voice_call(payload);
    }

    fn voice_call(&mut self, payload: Map<String, Value>) {
        let call = Call::Voice {
            host: self.target.host.clone(),
            session: self.target.session.clone(),
            payload: Value::Object(payload.clone()),
        };
        let mut request = payload;
        request.remove("audio");
        request.remove("video");
        let token = self.token(Pending::Voice {
            generation: self.generation,
            request,
            target: self.target.clone(),
            key: self.storage_key(),
        });
        self.effects.push(Effect::Call { token, call });
    }

    /// Discard (Cancel message for an uncertain send): asks before deleting
    /// a saved recording.
    pub fn discard(&mut self) -> Vec<Effect> {
        if matches!(self.state, State::Submitting | State::Saving) {
            return Vec::new();
        }
        if self.state == State::Uncertain && !self.target.draft_only {
            let mut payload = self.identity("cancel");
            payload.insert("message_id".into(), json!(self.id));
            self.state = State::Submitting;
            self.voice_call(payload);
            return self.take();
        }
        if !self.pcm.is_empty() {
            self.tokens += 1;
            let ticket = self.tokens;
            self.asks.insert(
                ticket,
                (
                    self.generation,
                    self.id.clone(),
                    self.target.clone(),
                    Map::new(),
                ),
            );
            self.effects.push(Effect::Ask(Ask::Discard {
                session: self.target.session.clone(),
                ticket,
            }));
            return self.take();
        }
        self.generation += 1;
        self.effects.push(Effect::CancelRecorder);
        self.send_after_saved = false;
        self.effects.push(Effect::StopPlayback);
        self.clear_recording();
        self.state = State::Idle;
        self.notice.clear();
        self.notice_kind = NoticeKind::General;
        if !self.loaded_key.is_empty() {
            self.clear(None);
        }
        self.take()
    }

    /// The discard question's answer: deletes only the recording it asked
    /// about.
    pub fn discard_confirmed(&mut self, ticket: u64, confirmed: bool) -> Vec<Effect> {
        let Some((generation, id, target, _)) = self.asks.remove(&ticket) else {
            return Vec::new();
        };
        let dictation_check = self.target.draft_only && self.state == State::Uncertain;
        if !confirmed
            || generation != self.generation
            || id != self.id
            || !self.target.same_conversation(&target)
            || (self.state != State::Preview && !dictation_check)
        {
            return Vec::new();
        }
        if self.target.draft_only {
            self.generation += 1;
            if self.state == State::Uncertain {
                let payload = json!({"action": "draft_cancel",
                    "machine_id": self.selected_profile.get("machine_id").cloned().unwrap_or(Value::Null),
                    "job_id": self.id});
                self.effects.push(Effect::Call {
                    token: 0,
                    call: Call::Speech { payload },
                });
            }
        }
        self.effects.push(Effect::StopPlayback);
        self.profile_generation += 1;
        self.profiles_loading = false;
        self.profiles_validated = false;
        self.state = State::Saving;
        self.clear(Some(Then::Discarded));
        self.take()
    }

    /// A draft save's answer.
    pub fn saved(&mut self, token: u64, ok: bool) -> Vec<Effect> {
        let Some(Pending::Save {
            generation,
            any_generation,
            then,
        }) = self.pending.remove(&token)
        else {
            return Vec::new();
        };
        if !any_generation && generation != self.generation {
            return Vec::new();
        }
        match then {
            Then::Selection => {
                if !ok {
                    self.notice = SAVE_SELECTION_FAILED.into();
                }
            }
            Then::Recorded { send_after } => {
                self.state = State::Preview;
                if !ok {
                    self.notice = SAVE_RECORDING_FAILED.into();
                }
                if ok && send_after {
                    self.send_now();
                }
            }
            Then::TranscriptionRequest => {
                if !ok {
                    self.state = State::Preview;
                    self.notice = SAVE_TRANSCRIPTION_FAILED.into();
                } else {
                    self.draft_request(true);
                }
            }
            Then::Send {
                call,
                previous_attempt,
                target,
                key,
            } => {
                if !ok {
                    if generation == self.generation {
                        self.attempted_rejection_generation = previous_attempt;
                        self.state = State::Preview;
                        self.notice = SAVE_REQUEST_FAILED.into();
                    }
                    return self.take();
                }
                // Explicit Send owns this target even if the composer moved
                // on while the checkpoint was written.
                let Call::Voice { payload, .. } = &*call else {
                    return self.take();
                };
                let mut request = payload.as_object().cloned().unwrap_or_default();
                request.remove("audio");
                request.remove("video");
                let token = self.token(Pending::Voice {
                    generation,
                    request,
                    target,
                    key,
                });
                self.effects.push(Effect::Call { token, call: *call });
            }
            Then::Refusal => {
                self.state = if ok { State::Preview } else { State::Uncertain };
                if !ok {
                    self.notice = SAVE_REFUSAL_FAILED.into();
                }
            }
            Then::Accepted => {
                if !ok {
                    self.state = State::Uncertain;
                    self.notice = RECEIPT_NOT_SAVED.into();
                } else {
                    self.clear_recording();
                    self.state = State::Idle;
                    self.notice.clear();
                }
            }
            Then::Discarded => {
                if !ok {
                    self.state = State::Preview;
                    self.notice = DISCARD_FAILED.into();
                } else {
                    self.clear_recording();
                    self.state = State::Idle;
                    self.notice.clear();
                }
            }
            Then::Transcript => {
                self.state = State::Preview;
                if ok {
                    self.apply_transcript();
                } else {
                    self.notice = SAVE_TRANSCRIPT_FAILED.into();
                }
            }
            Then::ReceiptCleared { id } => {
                if id != self.id {
                    return Vec::new();
                }
                if !ok {
                    self.state = State::Preview;
                    self.notice = RECEIPT_NOT_CLEARED.into();
                } else {
                    self.pcm.clear();
                    self.transcript.clear();
                    self.id.clear();
                    self.state = State::Idle;
                    self.notice.clear();
                }
            }
        }
        self.take()
    }

    /// A call's answer.
    pub fn answered(
        &mut self,
        token: u64,
        reply: Result<Map<String, Value>, String>,
    ) -> Vec<Effect> {
        match self.pending.remove(&token) {
            Some(Pending::Profiles {
                generation,
                request,
                target,
            }) => {
                if generation == self.generation && request == self.profile_generation {
                    self.profiles(reply, &target);
                }
            }
            Some(Pending::Voice {
                generation,
                request,
                target,
                key,
            }) => {
                if generation == self.generation {
                    self.result(reply, &request);
                } else if let Ok(reply) = reply {
                    let message = reply.get("message").cloned().unwrap_or(Value::Null);
                    let id_key = if request.contains_key("message_id") {
                        "message_id"
                    } else {
                        "request_id"
                    };
                    if message.get("id") == request.get(id_key)
                        && text(&message, "run_id") == target.run
                        && text(&message, "conversation_id") == target.conversation
                    {
                        self.effects.push(Effect::Message {
                            key: target.key.clone(),
                            message,
                        });
                        self.effects.push(Effect::Save {
                            token: 0,
                            key,
                            draft: StoredDraft::default(),
                        });
                    }
                }
            }
            Some(Pending::Draft { generation }) if generation == self.generation => {
                self.draft_result(reply);
            }
            _ => {}
        }
        self.take()
    }

    fn intent(&self) -> Value {
        let mut intent = json!({
            "speech": Value::Object(self.selected_profile.clone()),
            "audio": {"format": "pcm_s16le", "sample_rate": 16000, "channels": 1,
                      "bytes": self.pcm.len() + 44, "sha256": hex(&voice::wav(&self.pcm))},
        });
        if !self.video.is_empty() {
            let mut media = self.video_meta.clone();
            media.remove("poster_base64");
            media.insert("bytes".into(), json!(self.video.len()));
            media.insert("sha256".into(), json!(hex(&self.video)));
            if let Some(poster) = self.video_meta.get("poster_base64") {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(poster.as_str().unwrap_or(""))
                    .unwrap_or_default();
                media.insert("poster_sha256".into(), json!(hex(&bytes)));
            }
            intent["video"] = Value::Object(media);
        }
        intent
    }

    fn valid_rejection(
        &self,
        reply: &Map<String, Value>,
        proof: &Map<String, Value>,
        request: &Map<String, Value>,
    ) -> bool {
        const KEYS: [&str; 12] = [
            "schema",
            "state",
            "request_id",
            "message_id",
            "run_id",
            "conversation_id",
            "intent",
            "payload_sha256",
            "generation",
            "retry_token",
            "code",
            "detail",
        ];
        let keys: HashSet<&str> = proof.keys().map(String::as_str).collect();
        let generation = integer(proof.get("generation"));
        let old = integer(self.rejection.get("generation"));
        let uuid = |key: &str| {
            proof
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(canonical_uuid)
        };
        let field = |key: &str| proof.get(key).and_then(Value::as_str).unwrap_or("");
        let detail = field("detail");
        reply.get("schema").and_then(Value::as_f64) == Some(1.0)
            && keys == KEYS.into_iter().collect::<HashSet<_>>()
            && serde_json::to_vec(proof).is_ok_and(|bytes| bytes.len() <= 4096)
            && proof.get("schema").and_then(Value::as_f64) == Some(1.0)
            && field("state") == "rejected_before_acceptance"
            && uuid("request_id")
            && uuid("retry_token")
            && proof.get("request_id") == request.get("request_id")
            && field("message_id") == self.id
            && field("run_id") == self.target.run
            && field("conversation_id") == self.target.conversation
            && proof
                .get("intent")
                .is_some_and(|intent| intent.is_object() && json_equal(intent, &self.intent()))
            && digest(proof.get("payload_sha256"))
            && generation > self.attempted_rejection_generation
            && generation >= old
            && (self.rejection.is_empty()
                || (proof.get("payload_sha256") == self.rejection.get("payload_sha256")
                    && (generation > old
                        || proof.get("retry_token") == self.rejection.get("retry_token"))))
            && matches!(
                field("code"),
                "route_unavailable" | "storage_full" | "queue_full"
            )
            && !detail.is_empty()
            && detail.len() <= 2000
            && !detail.chars().any(char::is_control)
    }

    fn result(&mut self, reply: Result<Map<String, Value>, String>, request: &Map<String, Value>) {
        let id_key = if request.contains_key("message_id") {
            "message_id"
        } else {
            "request_id"
        };
        if self.id.is_empty()
            || request.get(id_key).and_then(Value::as_str) != Some(self.id.as_str())
        {
            return;
        }
        if request.get("action").and_then(Value::as_str) == Some("submit")
            && request.get("request_id").and_then(Value::as_str) != Some(self.operation_id.as_str())
        {
            return;
        }
        let (reply, error) = match reply {
            Ok(reply) => (reply, String::new()),
            Err(error) => (Map::new(), error),
        };
        let message = reply
            .get("message")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if error.is_empty() && message.is_empty() {
            if let Some(proof) = reply.get("rejection").and_then(Value::as_object) {
                if self.valid_rejection(&reply, proof, request) {
                    self.rejection = proof.clone();
                    self.state = State::Saving;
                    self.notice = REFUSED.into();
                    self.save(Then::Refusal, false);
                    return;
                }
            }
        }
        let message_value = Value::Object(message.clone());
        if !error.is_empty()
            || text(&message_value, "id") != self.id
            || text(&message_value, "run_id") != self.target.run
            || text(&message_value, "conversation_id") != self.target.conversation
        {
            self.state = State::Uncertain;
            self.notice = if error.is_empty() {
                UNCONFIRMED.into()
            } else {
                error
            };
            return;
        }
        if !self.video.is_empty() {
            let media = message_value.get("video").cloned().unwrap_or(Value::Null);
            let same = |key: &str| match (media.get(key), self.video_meta.get(key)) {
                (None, None) => true,
                (Some(a), Some(b)) => json_equal(a, b),
                _ => false,
            };
            if text(&media, "request_id") != self.id
                || media.get("index").and_then(Value::as_f64) != Some(1.0)
                || media.get("bytes").and_then(Value::as_f64) != Some(self.video.len() as f64)
                || text(&media, "mime") != "video/mp4"
                || !same("duration_ms")
                || !same("width")
                || !same("height")
            {
                self.state = State::Uncertain;
                self.notice = VIDEO_UNCONFIRMED.into();
                return;
            }
        }
        self.effects.push(Effect::Message {
            key: self.target.key.clone(),
            message: message_value,
        });
        self.clear(Some(Then::Accepted));
    }

    fn draft_request(&mut self, submit: bool) {
        if self.id.is_empty() || !self.target.draft_only {
            return;
        }
        let mut payload = Map::new();
        payload.insert(
            "action".into(),
            json!(if submit {
                "draft_submit"
            } else {
                "draft_status"
            }),
        );
        payload.insert(
            "machine_id".into(),
            self.selected_profile
                .get("machine_id")
                .cloned()
                .unwrap_or(Value::Null),
        );
        if submit {
            payload.insert("request_id".into(), json!(self.id));
            payload.insert(
                "profile_id".into(),
                self.selected_profile
                    .get("profile_id")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            if let Some(revision) = self.selected_profile.get("profile_revision") {
                payload.insert("profile_revision".into(), revision.clone());
            }
            payload.insert("audio".into(), voice::audio(&self.pcm));
        } else {
            payload.insert("job_id".into(), json!(self.id));
        }
        let token = self.token(Pending::Draft {
            generation: self.generation,
        });
        self.effects.push(Effect::Call {
            token,
            call: Call::Speech {
                payload: Value::Object(payload),
            },
        });
    }

    fn draft_result(&mut self, reply: Result<Map<String, Value>, String>) {
        if !self.target.draft_only || self.id.is_empty() {
            return;
        }
        let reply = match reply {
            Ok(reply)
                if reply.get("schema").and_then(Value::as_f64) == Some(1.0)
                    && reply.get("job_id").and_then(Value::as_str) == Some(self.id.as_str()) =>
            {
                Value::Object(reply)
            }
            Ok(_) => {
                self.state = State::Uncertain;
                self.notice = TRANSCRIPTION_UNCONFIRMED.into();
                return;
            }
            Err(error) => {
                self.state = State::Uncertain;
                self.notice = error;
                return;
            }
        };
        match text(&reply, "state") {
            "queued" | "running" => {
                self.draft_polls += 1;
                if self.draft_polls >= DRAFT_POLLS {
                    self.state = State::Uncertain;
                    self.notice = TRANSCRIPTION_PENDING.into();
                    return;
                }
                let token = self.token(Pending::Poll {
                    generation: self.generation,
                });
                self.effects.push(Effect::PollLater { token });
            }
            state => {
                let transcript = text(&reply, "text");
                if state != "completed"
                    || transcript.trim().is_empty()
                    || transcript.len() > MAX_TRANSCRIPT_BYTES
                {
                    self.state = State::Uncertain;
                    self.notice = NO_TRANSCRIPT.into();
                    return;
                }
                self.transcript = transcript.to_owned();
                self.state = State::Saving;
                self.save(Then::Transcript, false);
            }
        }
    }

    /// The poll timer fired.
    pub fn poll(&mut self, token: u64) -> Vec<Effect> {
        if let Some(Pending::Poll { generation }) = self.pending.remove(&token) {
            if generation == self.generation && self.state == State::Submitting {
                self.draft_request(false);
            }
        }
        self.take()
    }

    fn apply_transcript(&mut self) {
        if self.transcript.is_empty() || !self.current.same_conversation(&self.target) {
            return;
        }
        let token = self.token(Pending::Insert {
            generation: self.generation,
            id: self.id.clone(),
        });
        self.effects.push(Effect::Insert {
            token,
            text: self.transcript.clone(),
            id: self.id.clone(),
        });
    }

    /// The transcript's insertion into the draft.
    pub fn inserted(&mut self, token: u64, inserted: Inserted) -> Vec<Effect> {
        let Some(Pending::Insert { generation, id }) = self.pending.remove(&token) else {
            return Vec::new();
        };
        match inserted {
            Inserted::Refused => {
                self.state = State::Preview;
                self.notice = DRAFT_CHANGED.into();
            }
            _ if generation != self.generation || id != self.id => {}
            Inserted::Saved(false) => {
                self.state = State::Preview;
                self.notice = TRANSCRIPT_NOT_SAVED.into();
            }
            Inserted::Saved(true) => self.clear(Some(Then::ReceiptCleared { id })),
        }
        self.take()
    }

    /// Video or voice, from the mode switch; only while idle. The caller
    /// remembers it (`speech/video_mode`).
    pub fn can_switch_mode(&self) -> bool {
        !self.loading && self.state == State::Idle
    }

    /// The notice chip: the permission dialog, or its text.
    pub fn notice(&self) -> (NoticeKind, String) {
        let text = if self.notice.is_empty() {
            &self.profile_error
        } else {
            &self.notice
        };
        (self.notice_kind, text.clone())
    }
}

/// What the voice controls show.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Shown {
    pub key: String,
    pub id: String,
    pub state: State,
    /// The composer shows its recording state.
    pub recording: bool,
    pub hint: Option<String>,
    pub stop_preview: bool,
    pub video_preview: bool,
    pub voice_enabled: bool,
    pub voice_danger: bool,
    pub voice_icon: String,
    pub voice_label: String,
    pub voice_detail: String,
    pub mode_visible: bool,
    pub mode_enabled: bool,
    pub mode_icon: String,
    pub mode_label: String,
    pub cancel_visible: bool,
    pub preview: Option<String>,
    pub play_label: String,
    pub play_enabled: bool,
    pub discard_enabled: bool,
    pub discard_label: String,
    pub providers: Vec<Provider>,
    pub provider: Option<usize>,
    pub provider_enabled: bool,
    pub provider_placeholder: String,
    pub provider_tooltip: String,
    pub refresh_enabled: bool,
    pub refresh_label: String,
    pub send_label: String,
    pub send_enabled: bool,
    pub notice_visible: bool,
    pub notice_label: String,
    pub notice_detail: String,
}

/// What the controls show (`update`), with whether the preview plays.
pub fn shown(composer: &VoiceComposer, playing: bool) -> Shown {
    let c = composer;
    let current = &c.current;
    let recording = c.state == State::Recording;
    let video_mode = !current.draft_only && c.preferences.video_mode;
    let duration = voice::duration(c.seconds);
    let preview = !c.pcm.is_empty();
    let selected = c.selected_profile();
    let (notice_kind, notice_detail) = c.notice();
    Shown {
        key: current.key.clone(),
        id: c.id.clone(),
        state: c.state,
        recording: recording
            || c.state == State::Starting
            || (c.target.draft_only && c.state == State::Saving),
        hint: recording.then(|| {
            if c.preview_before_send {
                "Recording. Stop to preview. Space / Esc to stop"
            } else {
                "Recording. Send, or Stop to preview. Space / Esc to stop"
            }
            .to_owned()
        }),
        stop_preview: recording,
        video_preview: c.capture_video && (c.state == State::Starting || recording || preview),
        voice_enabled: !c.loading
            && (recording
                || (c.state == State::Idle
                    && current.available
                    && (current.draft_only || !current.conversation.is_empty())
                    && c.input_visible)),
        voice_danger: recording,
        voice_icon: if recording {
            if c.preview_before_send {
                "stop"
            } else {
                "send"
            }
        } else if video_mode {
            "video"
        } else {
            "microphone"
        }
        .into(),
        voice_label: if recording {
            if c.preview_before_send {
                format!("Stop ({duration})")
            } else {
                format!("Send ({duration})")
            }
        } else {
            match c.state {
                State::Starting => "Opening recorder…".into(),
                State::Saving => "Saving recording…".into(),
                _ if video_mode => "Video".into(),
                _ => "Voice".into(),
            }
        },
        voice_detail: if !recording && !current.available && !current.reason.is_empty() {
            current.reason.clone()
        } else if recording {
            if c.preview_before_send {
                "Stop recording and preview before sending."
            } else {
                "Send recording. Stop and preview or Space / Esc keeps it for review."
            }
            .into()
        } else if c.preferences.video_mode {
            "Click to start a video recording.".into()
        } else {
            "Click to start a voice recording.".into()
        },
        mode_visible: !current.draft_only,
        mode_enabled: c.can_switch_mode(),
        mode_icon: if c.preferences.video_mode {
            "microphone"
        } else {
            "video"
        }
        .into(),
        mode_label: if c.preferences.video_mode {
            "Switch to voice recording"
        } else {
            "Switch to video recording"
        }
        .into(),
        cancel_visible: c.state == State::Starting || recording,
        preview: preview.then(|| format!("Voice {duration}")),
        play_label: if playing { "Pause" } else { "Play" }.into(),
        play_enabled: preview && c.state != State::Submitting,
        discard_enabled: matches!(c.state, State::Preview | State::Uncertain),
        discard_label: if c.state == State::Uncertain && !current.draft_only {
            "Cancel message"
        } else {
            "Discard"
        }
        .into(),
        providers: c.providers.clone(),
        provider: c.current_provider,
        provider_enabled: c.state == State::Preview
            && c.rejection.is_empty()
            && c.profiles_validated
            && !c.profiles_loading,
        provider_placeholder: if c.profiles_loading {
            "Checking providers…"
        } else {
            "Choose a voice provider"
        }
        .into(),
        provider_tooltip: c.profile_error.clone(),
        refresh_enabled: c.state == State::Preview && !c.profiles_loading,
        refresh_label: if c.profiles_loading {
            "Checking…"
        } else {
            "Refresh"
        }
        .into(),
        send_label: if current.draft_only {
            match c.state {
                State::Uncertain => "Check transcription",
                State::Submitting => "Transcribing…",
                _ if c.transcript.is_empty() => "Transcribe",
                _ => "Add to draft",
            }
        } else {
            match c.state {
                State::Uncertain => "Check message",
                State::Submitting => "Sending voice…",
                _ => "Send voice",
            }
        }
        .into(),
        send_enabled: c.state == State::Uncertain
            || (c.state == State::Preview
                && current.available
                && current.same_conversation(&c.target)
                && ((current.draft_only && !c.transcript.is_empty()) || !selected.is_empty())
                && (c.video.is_empty() || c.video_supported)),
        notice_visible: !notice_detail.is_empty(),
        notice_label: if notice_kind == NoticeKind::MicrophonePermission {
            "Microphone access"
        } else {
            "Voice notice"
        }
        .into(),
        notice_detail,
    }
}

/// The platform whose settings a permission text names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

/// The microphone recovery dialog's text.
pub fn permission_text(platform: Platform) -> &'static str {
    match platform {
        Platform::Macos => "Allow Zerus to use the microphone in System Settings > Privacy & Security > Microphone. Then return to Zerus and press Voice again.",
        Platform::Windows => "Open Settings > Privacy & security > Microphone. Enable Microphone access and Let desktop apps access your microphone. Then return to Zerus and press Voice again.",
        Platform::Linux => "Open your desktop's Settings and check microphone permissions for Zerus. Then return to Zerus and press Voice again.",
    }
}

/// The system settings page for the microphone, if the platform has one.
pub fn microphone_settings_url(platform: Platform) -> Option<&'static str> {
    match platform {
        Platform::Macos => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        }
        Platform::Windows => Some("ms-settings:privacy-microphone"),
        Platform::Linux => None,
    }
}

/// Runs a speech or voice call as the Qt client does: `hgs --help` first
/// (remembered per computer in `supported`), then the call with its
/// payload on stdin. Blocks; run it off the UI thread.
pub fn run_call(
    hgs: &Path,
    call: &Call,
    supported: &mut HashSet<String>,
) -> Result<Map<String, Value>, String> {
    let (host, session, payload) = match call {
        Call::Voice {
            host,
            session,
            payload,
        } => (host.as_str(), Some(session.as_str()), payload),
        Call::Speech { payload } => ("", None, payload),
    };
    if session.is_some_and(str::is_empty) {
        return Err("Choose a conversation before sending voice.".into());
    }
    let computer = ComputerId::from(host);
    let manage = voice::manages(text(payload, "action"));
    let cache = format!(
        "{}\n{host}",
        if session.is_some() {
            "voice"
        } else if manage {
            "manage"
        } else {
            "speech"
        }
    );
    if !supported.contains(&cache) {
        let exit = crate::run::run(hgs, &voice::help_invocation(&computer));
        if let Some(error) = voice::failure(&exit, manage && session.is_none()) {
            return Err(error);
        }
        let Exit::Code { stdout, .. } = exit else {
            return Err("The speech computer returned an invalid response.".into());
        };
        let help = String::from_utf8_lossy(&stdout);
        match session {
            Some(_) => voice::voice_supported(&help)?,
            None => voice::speech_supported(&help, manage)?,
        }
        supported.insert(cache);
    }
    let invocation = match session {
        Some(session) => voice::voice_invocation(&computer, session, payload)?,
        None => voice::speech_invocation(&computer, payload),
    };
    voice::reply(
        crate::run::run(hgs, &invocation),
        manage && session.is_none(),
    )
}
