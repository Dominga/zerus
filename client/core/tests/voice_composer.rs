//! The voice composer against `tray/src/speech/SpeechComposer.cpp`.
use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use zerus_client_core::attachments::{Attachment, Draft};
use zerus_client_core::drafts::StoredDraft;
use zerus_client_core::voice;
use zerus_client_core::voice_composer::{self, *};

const MACHINE: &str = "11111111-2222-4333-8444-555555555555";

fn id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

struct Driver {
    composer: VoiceComposer,
    saves: VecDeque<(u64, String, StoredDraft)>,
    calls: VecDeque<(u64, Call)>,
    loads: VecDeque<(u64, String)>,
    inserts: VecDeque<(u64, String, String)>,
    polls: VecDeque<u64>,
    asks: VecDeque<Ask>,
    other: Vec<Effect>,
}

fn target() -> Target {
    Target {
        key: "local\nclaude/p/fix".into(),
        host: String::new(),
        session: "claude/p/fix".into(),
        run: "run-1".into(),
        conversation: "conv-1".into(),
        available: true,
        ..Target::default()
    }
}

fn preferences() -> Preferences {
    Preferences {
        machine_id: MACHINE.into(),
        profile_id: "local".into(),
        ..Preferences::default()
    }
}

impl Driver {
    fn new(target: Target, preferences: Preferences) -> Driver {
        let next = Arc::new(AtomicU64::new(0));
        let mut driver = Driver {
            composer: VoiceComposer::new(move || id(next.fetch_add(1, Ordering::SeqCst) + 1)),
            saves: VecDeque::new(),
            calls: VecDeque::new(),
            loads: VecDeque::new(),
            inserts: VecDeque::new(),
            polls: VecDeque::new(),
            asks: VecDeque::new(),
            other: Vec::new(),
        };
        let effects = driver.composer.update(target, true, preferences);
        driver.take(effects);
        let (token, _) = driver.loads.pop_front().unwrap();
        let effects = driver.composer.loaded(token, None);
        driver.take(effects);
        driver
    }

    fn take(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Save { token, key, draft } => self.saves.push_back((token, key, draft)),
                Effect::Call { token, call } => self.calls.push_back((token, call)),
                Effect::Load { token, key } => self.loads.push_back((token, key)),
                Effect::Insert { token, text, id } => self.inserts.push_back((token, text, id)),
                Effect::PollLater { token } => self.polls.push_back(token),
                Effect::Ask(ask) => self.asks.push_back(ask),
                other => self.other.push(other),
            }
        }
    }

    fn save(&mut self, ok: bool) -> StoredDraft {
        let (token, _, draft) = self.saves.pop_front().expect("a save");
        let effects = self.composer.saved(token, ok);
        self.take(effects);
        draft
    }

    fn call(&mut self) -> (u64, Call) {
        self.calls.pop_front().expect("a call")
    }

    fn answer(&mut self, token: u64, reply: Result<Value, String>) {
        let reply = reply.map(|value| value.as_object().cloned().unwrap());
        let effects = self.composer.answered(token, reply);
        self.take(effects);
    }

    fn profiles(&mut self, reply: Value) {
        let (token, call) = self.call();
        assert!(
            matches!(&call, Call::Voice { payload, .. } | Call::Speech { payload } if payload["action"].as_str().unwrap().ends_with("profiles"))
        );
        self.answer(token, Ok(reply));
    }

    fn shown(&self) -> Shown {
        voice_composer::shown(&self.composer, false)
    }
}

fn providers() -> Value {
    json!({"schema": 1, "features": ["voice_video"], "computers": [{"machine_id": MACHINE, "name": "arch",
        "profiles": [{"id": "local", "name": "Whisper", "available": true, "profile_revision": 3},
                     {"id": "cloud", "available": true}]}]})
}

fn pcm(seconds: usize) -> Vec<u8> {
    vec![1u8; seconds * 32000]
}

fn accepted(message: &str) -> Value {
    json!({"schema": 1, "message": {"id": message, "run_id": "run-1", "conversation_id": "conv-1"}})
}

#[test]
fn send_while_recording_saves_then_sends_once() {
    let mut driver = Driver::new(target(), preferences());
    let effects = driver.composer.press();
    driver.take(effects);
    assert!(driver.other.contains(&Effect::Record {
        video: false,
        microphone: String::new()
    }));
    assert_eq!(driver.composer.state(), State::Starting);
    driver.profiles(providers());
    let effects = driver.composer.started();
    driver.take(effects);
    driver.composer.elapsed(2);
    let shown = driver.shown();
    assert_eq!(shown.voice_label, "Send (0:02)");
    assert_eq!(
        shown.hint.as_deref(),
        Some("Recording. Send, or Stop to preview. Space / Esc to stop")
    );
    assert_eq!(shown.providers[0].label, "arch / Whisper");
    // Send while recording: stop, save, then send.
    let effects = driver.composer.press();
    driver.take(effects);
    assert!(driver
        .other
        .contains(&Effect::StopRecorder { video: false }));
    let effects = driver.composer.recorded(pcm(2), None);
    driver.take(effects);
    assert!(driver.calls.is_empty());
    let recording = driver.save(true);
    assert_eq!(recording.label, "Voice message (claude/p/fix)");
    assert_eq!(recording.draft.attachments[0].name, "voice.wav");
    assert_eq!(recording.form_state["submitted"], false);
    assert_eq!(recording.form_state["kind"], "voice_message");
    // The submit is checkpointed before anything is sent.
    assert!(driver.calls.is_empty());
    let checkpoint = driver.save(true);
    assert_eq!(checkpoint.form_state["submitted"], true);
    assert_eq!(driver.composer.state(), State::Submitting);
    let message = driver.composer.id().to_owned();
    let (token, call) = driver.call();
    let Call::Voice {
        session, payload, ..
    } = call
    else {
        panic!("voice call")
    };
    assert_eq!(session, "claude/p/fix");
    assert_eq!(payload["action"], "submit");
    assert_eq!(payload["request_id"], message.as_str());
    assert_eq!(payload["expected_run_id"], "run-1");
    assert_eq!(
        payload["speech"],
        json!({"machine_id": MACHINE, "profile_id": "local", "profile_revision": 3})
    );
    assert_eq!(payload["audio"], voice::audio(&pcm(2)));
    driver.answer(token, Ok(accepted(&message)));
    assert!(driver
        .other
        .iter()
        .any(|effect| matches!(effect, Effect::Message { .. })));
    let cleared = driver.save(true);
    assert!(cleared.draft.attachments.is_empty());
    assert_eq!(driver.composer.state(), State::Idle);
}

#[test]
fn without_a_confirmed_provider_send_keeps_the_recording_for_review() {
    let mut driver = Driver::new(target(), Preferences::default());
    let effects = driver.composer.press();
    driver.take(effects);
    let effects = driver.composer.started();
    driver.take(effects);
    // Providers are still loading: Send stops to preview instead.
    let effects = driver.composer.press();
    driver.take(effects);
    let effects = driver.composer.recorded(pcm(1), None);
    driver.take(effects);
    driver.save(true);
    assert_eq!(driver.composer.state(), State::Preview);
    assert!(driver.saves.is_empty() && driver.calls.len() == 1);
    assert_eq!(
        driver.composer.notice().1,
        "Review the transcription provider before sending. The recording is kept."
    );
}

fn preview() -> Driver {
    let mut driver = Driver::new(
        target(),
        Preferences {
            preview_before_send: true,
            ..preferences()
        },
    );
    let effects = driver.composer.press();
    driver.take(effects);
    driver.profiles(providers());
    let effects = driver.composer.started();
    driver.take(effects);
    assert_eq!(driver.shown().voice_label, "Stop (0:00)");
    let effects = driver.composer.stop_recording(false);
    driver.take(effects);
    assert_eq!(driver.composer.state(), State::Saving);
    let effects = driver.composer.recorded(pcm(3), None);
    driver.take(effects);
    driver.save(true);
    assert_eq!(driver.composer.state(), State::Preview);
    driver
}

#[test]
fn playback_reads_only_the_exact_saved_recording_without_any_delivery_effect() {
    let mut driver = preview();
    let id = driver.composer.id().to_owned();
    assert_eq!(driver.shown().key, target().key);
    assert_eq!(driver.shown().id, id);
    let before = (driver.saves.len(), driver.calls.len());
    let saved = driver.composer.preview(&id).unwrap();
    assert_eq!(saved.pcm, pcm(3));
    assert!(saved.video.is_empty());
    assert_eq!(saved.id, id);
    assert!(driver.composer.preview("").is_none());
    assert!(driver.composer.preview("another-recording").is_none());
    assert_eq!(before, (driver.saves.len(), driver.calls.len()));
    let effects = driver.composer.send();
    driver.take(effects);
    assert!(
        driver.composer.preview(&id).is_none(),
        "delivery locks playback"
    );
}

#[test]
fn preview_then_send_and_a_lost_answer_is_only_checked() {
    let mut driver = preview();
    let shown = driver.shown();
    assert_eq!(shown.preview.as_deref(), Some("Voice 0:03"));
    assert!(shown.send_enabled);
    assert_eq!(shown.send_label, "Send voice");
    let effects = driver.composer.send();
    driver.take(effects);
    driver.save(true);
    let (token, _) = driver.call();
    driver.answer(token, Err("connection lost".into()));
    assert_eq!(driver.composer.state(), State::Uncertain);
    assert_eq!(driver.composer.notice().1, "connection lost");
    let shown = driver.shown();
    assert_eq!(
        (shown.send_label.as_str(), shown.discard_label.as_str()),
        ("Check message", "Cancel message")
    );
    // Check reads the message's status; it never sends the audio again.
    let effects = driver.composer.send();
    driver.take(effects);
    let (token, call) = driver.call();
    let Call::Voice { payload, .. } = call else {
        panic!()
    };
    assert_eq!(payload["action"], "status");
    assert_eq!(payload["message_id"], driver.composer.id());
    assert!(payload.get("audio").is_none());
    let message = driver.composer.id().to_owned();
    driver.answer(token, Ok(accepted(&message)));
    driver.save(true);
    assert_eq!(driver.composer.state(), State::Idle);
}

#[test]
fn an_interrupted_send_reopens_uncertain_and_cancel_asks_the_computer() {
    let mut first = preview();
    let effects = first.composer.send();
    first.take(effects);
    let checkpoint = first.save(true);
    // The app quit here. The saved draft reopens uncertain.
    let next = Arc::new(AtomicU64::new(100));
    let mut composer = VoiceComposer::new(move || id(next.fetch_add(1, Ordering::SeqCst)));
    let effects = composer.update(target(), true, preferences());
    let token = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Load { token, key } => {
                assert_eq!(key, "voice-message-draft\nlocal\nclaude/p/fix");
                Some(*token)
            }
            _ => None,
        })
        .unwrap();
    let effects = composer.loaded(token, Some(checkpoint));
    assert_eq!(composer.state(), State::Uncertain);
    assert_eq!(composer.notice().1, voice_composer::INTERRUPTED);
    assert!(effects.iter().all(|effect| match effect {
        Effect::Call {
            call: Call::Voice { payload, .. },
            ..
        } => payload["action"] == "profiles",
        _ => true,
    }));
    let effects = composer.discard();
    let cancel = effects.iter().find_map(|effect| match effect {
        Effect::Call {
            call: Call::Voice { payload, .. },
            ..
        } => Some(payload.clone()),
        _ => None,
    });
    assert_eq!(cancel.unwrap()["action"], "cancel");
    assert_eq!(composer.state(), State::Submitting);
}

#[test]
fn a_cold_cache_asks_and_a_changed_composer_sends_nothing() {
    let mut driver = preview();
    let warm = target();
    let cold = Target {
        warning: "The prompt cache expired.".into(),
        ..warm.clone()
    };
    let effects = driver.composer.update(cold, true, preferences());
    driver.take(effects);
    let effects = driver.composer.send();
    driver.take(effects);
    let Some(Ask::ColdCache { warning, ticket }) = driver.asks.pop_front() else {
        panic!()
    };
    assert_eq!(warning, "The prompt cache expired.");
    let effects = driver.composer.send_confirmed(ticket, false);
    driver.take(effects);
    assert!(driver.saves.is_empty() && driver.composer.state() == State::Preview);
    let effects = driver.composer.send();
    driver.take(effects);
    let Some(Ask::ColdCache { ticket, .. }) = driver.asks.pop_front() else {
        panic!()
    };
    let effects = driver.composer.send_confirmed(ticket, true);
    driver.take(effects);
    assert_eq!(driver.composer.state(), State::Submitting);
    // A ticket is answered once.
    assert!(driver.composer.send_confirmed(ticket, true).is_empty());
}

#[test]
fn discarding_a_saved_recording_asks_first() {
    let mut driver = preview();
    let effects = driver.composer.discard();
    driver.take(effects);
    let Some(Ask::Discard { session, ticket }) = driver.asks.pop_front() else {
        panic!()
    };
    assert_eq!(session, "claude/p/fix");
    assert!(driver.saves.is_empty());
    let effects = driver.composer.discard_confirmed(ticket, true);
    driver.take(effects);
    let cleared = driver.save(true);
    assert!(cleared.draft.attachments.is_empty());
    assert_eq!(driver.composer.state(), State::Idle);
}

fn rejection(request: &str, message: &str, intent: Value, generation: i64) -> Value {
    json!({"schema": 1, "rejection": {"schema": 1, "state": "rejected_before_acceptance",
        "request_id": request, "message_id": message, "run_id": "run-1", "conversation_id": "conv-1",
        "intent": intent, "payload_sha256": "a".repeat(64), "generation": generation,
        "retry_token": "abcdef00-0000-4000-8000-000000000001", "code": "queue_full", "detail": "Queue is full."}})
}

#[test]
fn a_refusal_before_acceptance_retries_the_same_recording_and_provider() {
    let mut driver = preview();
    let effects = driver.composer.send();
    driver.take(effects);
    driver.save(true);
    let (token, call) = driver.call();
    let Call::Voice { payload, .. } = call else {
        panic!()
    };
    let message = driver.composer.id().to_owned();
    let wav = voice::wav(&pcm(3));
    let intent = json!({"speech": {"machine_id": MACHINE, "profile_id": "local", "profile_revision": 3},
        "audio": {"format": "pcm_s16le", "sample_rate": 16000, "channels": 1, "bytes": wav.len(),
                  "sha256": sha256(&wav)}});
    // A proof for another recording is not a refusal: the outcome is unknown.
    let mut wrong = intent.clone();
    wrong["audio"]["bytes"] = json!(1);
    driver.answer(
        token,
        Ok(rejection(
            payload["request_id"].as_str().unwrap(),
            &message,
            wrong,
            1,
        )),
    );
    assert_eq!(driver.composer.state(), State::Uncertain);

    let mut driver = preview();
    let effects = driver.composer.send();
    driver.take(effects);
    driver.save(true);
    let (token, call) = driver.call();
    let Call::Voice { payload, .. } = call else {
        panic!()
    };
    let message = driver.composer.id().to_owned();
    driver.answer(
        token,
        Ok(rejection(
            payload["request_id"].as_str().unwrap(),
            &message,
            intent,
            1,
        )),
    );
    assert_eq!(driver.composer.state(), State::Saving);
    let saved = driver.save(true);
    assert_eq!(saved.form_state["rejection"]["generation"], 1);
    assert_eq!(driver.composer.state(), State::Preview);
    assert_eq!(driver.composer.notice().1, voice_composer::REFUSED);
    // The provider stays bound to the refused request.
    assert!(!driver.shown().provider_enabled);
    assert!(driver.composer.choose_provider(1).is_empty());
    let effects = driver.composer.send();
    driver.take(effects);
    driver.save(true);
    let (_, call) = driver.call();
    let Call::Voice { payload, .. } = call else {
        panic!()
    };
    assert_eq!(payload["message_id"], message.as_str());
    assert_ne!(payload["request_id"], message.as_str());
    assert_eq!(payload["rejection"]["generation"], 1);
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn an_answer_after_the_composer_moved_on_still_records_the_message() {
    let mut driver = preview();
    let effects = driver.composer.send();
    driver.take(effects);
    driver.save(true);
    let (token, _) = driver.call();
    let message = driver.composer.id().to_owned();
    let other = Target {
        key: "local\nclaude/p/other".into(),
        session: "claude/p/other".into(),
        ..target()
    };
    let effects = driver.composer.update(other, true, preferences());
    driver.take(effects);
    driver.answer(token, Ok(accepted(&message)));
    assert!(driver.other.iter().any(
        |effect| matches!(effect, Effect::Message { key, .. } if key == "local\nclaude/p/fix")
    ));
    let (token, key, draft) = driver.saves.pop_back().unwrap();
    assert_eq!(
        (token, key.as_str()),
        (0, "voice-message-draft\nlocal\nclaude/p/fix")
    );
    assert!(draft.draft.attachments.is_empty());
}

#[test]
fn providers_follow_the_saved_choice_and_its_revision() {
    // A profile ID alone matches when it is unique.
    let mut driver = Driver::new(
        target(),
        Preferences {
            profile_id: "cloud".into(),
            ..Preferences::default()
        },
    );
    let effects = driver.composer.press();
    driver.take(effects);
    driver.profiles(providers());
    assert_eq!(driver.shown().provider, Some(1));
    // An unknown preferred provider shows the placeholder and the reason.
    let mut driver = Driver::new(
        target(),
        Preferences {
            machine_id: MACHINE.into(),
            profile_id: "gone".into(),
            ..Preferences::default()
        },
    );
    let effects = driver.composer.press();
    driver.take(effects);
    driver.profiles(providers());
    let shown = driver.shown();
    assert_eq!(
        shown.providers.last().unwrap().label,
        "Selected provider (unavailable)"
    );
    assert_eq!(shown.provider, Some(2));
    assert_eq!(shown.provider_tooltip, voice_composer::PROVIDER_CHANGED);
    assert!(driver.composer.selected_profile().is_empty());
    // Invalid machines, profile IDs and revisions are skipped.
    let mut driver = Driver::new(target(), Preferences::default());
    let effects = driver.composer.press();
    driver.take(effects);
    driver.profiles(json!({"schema": 1, "computers": [
        {"machine_id": "not-a-uuid", "name": "x", "profiles": [{"id": "a", "available": true}]},
        {"machine_id": MACHINE, "name": "arch", "profiles": [
            {"id": "bad id", "available": true}, {"id": "r", "available": true, "profile_revision": 1.5},
            {"id": "off", "available": false}, {"id": "ok", "available": true}]}]}));
    let shown = driver.shown();
    assert_eq!(shown.providers.len(), 1);
    assert_eq!(shown.providers[0].label, "arch / ok");
    assert_eq!(shown.provider, Some(0));
}

#[test]
fn a_video_needs_a_computer_that_takes_video() {
    let mut driver = Driver::new(
        target(),
        Preferences {
            video_mode: true,
            ..preferences()
        },
    );
    assert_eq!(driver.shown().voice_label, "Video");
    let effects = driver.composer.press();
    driver.take(effects);
    assert!(driver.other.contains(&Effect::Record {
        video: true,
        microphone: String::new()
    }));
    let mut older = providers();
    older["features"] = json!([]);
    driver.profiles(older);
    assert_eq!(
        driver.shown().provider_tooltip,
        voice_composer::VIDEO_NEEDS_UPDATE
    );
}

#[test]
fn dictation_transcribes_into_the_draft() {
    let draft_target = Target {
        key: "draft\ntask-1".into(),
        draft_only: true,
        run: String::new(),
        conversation: String::new(),
        ..target()
    };
    let mut driver = Driver::new(draft_target, preferences());
    assert!(!driver.shown().mode_visible);
    let effects = driver.composer.press();
    driver.take(effects);
    let (token, call) = driver.call();
    assert_eq!(
        call,
        Call::Speech {
            payload: json!({"action": "draft_profiles"})
        }
    );
    driver.answer(token, Ok(providers()));
    let effects = driver.composer.started();
    driver.take(effects);
    // Dictation always previews.
    assert_eq!(driver.shown().voice_label, "Stop (0:00)");
    let effects = driver.composer.press();
    driver.take(effects);
    let effects = driver.composer.recorded(pcm(1), None);
    driver.take(effects);
    driver.save(true);
    assert_eq!(driver.shown().send_label, "Transcribe");
    let effects = driver.composer.send();
    driver.take(effects);
    driver.save(true);
    let (token, call) = driver.call();
    let Call::Speech { payload } = call else {
        panic!()
    };
    assert_eq!(payload["action"], "draft_submit");
    assert_eq!(payload["profile_revision"], 3);
    let job = driver.composer.id().to_owned();
    driver.answer(
        token,
        Ok(json!({"schema": 1, "job_id": job, "state": "running"})),
    );
    let poll = driver.polls.pop_front().unwrap();
    let effects = driver.composer.poll(poll);
    driver.take(effects);
    let (token, call) = driver.call();
    assert!(matches!(call, Call::Speech { ref payload } if payload["action"] == "draft_status"));
    driver.answer(
        token,
        Ok(json!({"schema": 1, "job_id": job, "state": "completed", "text": "hello there"})),
    );
    driver.save(true);
    let (token, text, inserted_id) = driver.inserts.pop_front().unwrap();
    assert_eq!(
        (text.as_str(), inserted_id.as_str()),
        ("hello there", job.as_str())
    );
    let effects = driver.composer.inserted(token, Inserted::Saved(true));
    driver.take(effects);
    driver.save(true);
    assert_eq!(driver.composer.state(), State::Idle);
}

#[test]
fn a_draft_that_changed_keeps_the_transcript_for_add_to_draft() {
    let draft_target = Target {
        key: "draft\ntask-1".into(),
        draft_only: true,
        ..target()
    };
    let mut driver = Driver::new(draft_target, preferences());
    let effects = driver.composer.press();
    driver.take(effects);
    let (token, _) = driver.call();
    driver.answer(token, Ok(providers()));
    let effects = driver.composer.started();
    driver.take(effects);
    let effects = driver.composer.press();
    driver.take(effects);
    let effects = driver.composer.recorded(pcm(1), None);
    driver.take(effects);
    driver.save(true);
    let effects = driver.composer.send();
    driver.take(effects);
    driver.save(true);
    let (token, _) = driver.call();
    let job = driver.composer.id().to_owned();
    driver.answer(
        token,
        Ok(json!({"schema": 1, "job_id": job, "state": "completed", "text": "hi"})),
    );
    driver.save(true);
    let (token, _, _) = driver.inserts.pop_front().unwrap();
    let effects = driver.composer.inserted(token, Inserted::Refused);
    driver.take(effects);
    assert_eq!(driver.composer.notice().1, voice_composer::DRAFT_CHANGED);
    assert_eq!(driver.shown().send_label, "Add to draft");
}

#[test]
fn a_denied_microphone_offers_its_settings() {
    let mut driver = Driver::new(target(), preferences());
    let effects = driver.composer.press();
    driver.take(effects);
    let effects = driver
        .composer
        .failed(Failure::PermissionDenied, "Microphone access was denied.");
    driver.take(effects);
    assert!(driver.other.contains(&Effect::PermissionRecovery));
    assert_eq!(driver.composer.state(), State::Idle);
    let shown = driver.shown();
    assert_eq!(shown.notice_label, "Microphone access");
    assert_eq!(shown.notice_detail, "Microphone access was denied.");
    assert_eq!(
        microphone_settings_url(Platform::Macos),
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
    );
    assert_eq!(microphone_settings_url(Platform::Linux), None);
    assert!(permission_text(Platform::Windows)
        .starts_with("Open Settings > Privacy & security > Microphone."));
}

#[test]
fn recordings_are_checked() {
    let mut driver = Driver::new(target(), preferences());
    let effects = driver.composer.press();
    driver.take(effects);
    let effects = driver.composer.started();
    driver.take(effects);
    let effects = driver.composer.stop_recording(false);
    driver.take(effects);
    let effects = driver.composer.recorded(vec![0; 3199], None);
    driver.take(effects);
    assert_eq!(
        driver.composer.notice().1,
        "The voice recording is invalid."
    );
    assert_eq!(driver.composer.state(), State::Idle);
    // A saved draft that is not a WAV recording is reported, not loaded.
    let next = Arc::new(AtomicU64::new(1));
    let mut composer = VoiceComposer::new(move || id(next.fetch_add(1, Ordering::SeqCst)));
    let effects = composer.update(target(), true, preferences());
    let token = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Load { token, .. } => Some(*token),
            _ => None,
        })
        .unwrap();
    let broken = StoredDraft {
        draft: Draft {
            attachments: vec![Attachment {
                name: "voice.wav".into(),
                mime: "audio/wav".into(),
                bytes: Arc::from(vec![0u8; 4000]),
                reference: String::new(),
            }],
            ..Draft::default()
        },
        form_state: Map::new(),
        ..StoredDraft::default()
    };
    composer.loaded(token, Some(broken));
    assert_eq!(composer.notice().1, "The saved voice recording is invalid.");
    assert_eq!(composer.state(), State::Idle);
}

#[test]
fn numbers_compare_by_value_as_qt_does() {
    assert!(json_equal(&json!({"a": 1}), &json!({"a": 1.0})));
    assert!(!json_equal(&json!({"a": 1}), &json!({"a": 1, "b": 2})));
    assert!(!json_equal(&json!([1, 2]), &json!([2, 1])));
}

#[cfg(unix)]
mod calls {
    use super::*;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicUsize;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct Fake {
        dir: PathBuf,
        hgs: PathBuf,
    }

    impl Fake {
        fn new(config: Value) -> Fake {
            let dir = std::env::temp_dir().join(format!(
                "zerus-fake-hgs-voice-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let hgs = dir.join("hgs");
            let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
            // A symlink, never a copy (see tests/actions.rs).
            std::os::unix::fs::symlink(manifest.join("tests/support/fake_hgs_voice.py"), &hgs)
                .unwrap();
            std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
            Fake { dir, hgs }
        }

        fn log(&self) -> Vec<Value> {
            std::fs::read_to_string(self.dir.join("log.ndjson"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn a_voice_call_probes_once_per_computer_and_sends_its_payload() {
        let fake = Fake::new(
            json!({"help": "usage: hgs voice NAME [--json]\n", "reply": {"schema": 1, "ok": true}}),
        );
        let mut supported = HashSet::new();
        let call = Call::Voice {
            host: "box".into(),
            session: "claude/p/fix".into(),
            payload: json!({"action": "status", "message_id": "m"}),
        };
        for _ in 0..2 {
            let reply = run_call(&fake.hgs, &call, &mut supported).unwrap();
            assert_eq!(reply["ok"], true);
        }
        let log = fake.log();
        assert_eq!(log.len(), 3, "one probe, two calls");
        assert_eq!(log[0]["argv"], json!(["@box", "--help"]));
        assert_eq!(
            log[1]["argv"],
            json!(["@box", "voice", "claude/p/fix", "--json"])
        );
        assert_eq!(
            serde_json::from_str::<Value>(log[1]["stdin"].as_str().unwrap()).unwrap(),
            json!({"action": "status", "message_id": "m"})
        );
        // An older CLI is named, and nothing is sent.
        let older = Fake::new(json!({"help": "usage: hgs", "reply": {"schema": 1}}));
        assert_eq!(
            run_call(&older.hgs, &call, &mut HashSet::new()),
            Err("Voice messages need an updated hgs CLI on the conversation computer.".into())
        );
        assert_eq!(older.log().len(), 1);
    }
}

#[test]
fn an_explicit_send_goes_out_after_its_checkpoint_even_if_the_composer_moved_on() {
    let mut driver = preview();
    let effects = driver.composer.send();
    driver.take(effects);
    let other = Target {
        key: "local\nclaude/p/other".into(),
        session: "claude/p/other".into(),
        ..target()
    };
    let effects = driver.composer.update(other, true, preferences());
    driver.take(effects);
    driver.loads.clear();
    driver.save(true);
    let (_, call) = driver.call();
    let Call::Voice {
        session, payload, ..
    } = call
    else {
        panic!()
    };
    assert_eq!(session, "claude/p/fix");
    assert_eq!(payload["action"], "submit");
}

#[test]
fn a_recording_that_could_not_be_saved_is_never_sent() {
    let mut driver = Driver::new(target(), preferences());
    let effects = driver.composer.press();
    driver.take(effects);
    driver.profiles(providers());
    let effects = driver.composer.started();
    driver.take(effects);
    let effects = driver.composer.press();
    driver.take(effects);
    let effects = driver.composer.recorded(pcm(1), None);
    driver.take(effects);
    driver.save(false);
    assert!(driver.saves.is_empty() && driver.calls.is_empty());
    assert_eq!(driver.composer.state(), State::Preview);
    assert_eq!(
        driver.composer.notice().1,
        voice_composer::SAVE_RECORDING_FAILED
    );
}
