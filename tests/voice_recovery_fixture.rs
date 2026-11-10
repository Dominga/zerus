//! Exact-child caller fixture for test_speech_recovery.py. Only capture and
//! timer/reply delivery are controlled; policies, stores, CLI calls and receipts are real.
#![cfg(unix)]
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};
use zerus_client_core::{
    drafts::{DraftStore, StoredDraft},
    settings::Settings,
    speech_settings::{
        self, CallId, Defaults, EditorForm, Effect as ProviderEffect, Platform, Providers,
        PENDING_KEY,
    },
    voice_composer::{self, Call, Effect, Preferences, Target, VoiceComposer},
};

fn load(store: &DraftStore, key: &str) -> StoredDraft {
    let (tx, rx) = mpsc::channel();
    store.load(key, move |v| {
        tx.send(v).unwrap();
    });
    let value = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(value.storage_error.is_none());
    value.draft
}

fn audit(calls: &mut Vec<Value>, payload: &Value) {
    assert!(calls.len() < 128, "bounded caller fixture");
    let mut record = serde_json::Map::new();
    for key in [
        "action",
        "request_id",
        "job_id",
        "operation_id",
        "machine_id",
        "expected_machine_id",
        "profile_id",
        "profile_revision",
        "expected_revision",
        "expected_profile_revision",
    ] {
        if let Some(v) = payload.get(key) {
            record.insert(key.into(), v.clone());
        }
    }
    calls.push(Value::Object(record));
}

fn voice_step(
    composer: &mut VoiceComposer,
    store: &DraftStore,
    hgs: &Path,
    known: &mut HashSet<String>,
    calls: &mut Vec<Value>,
    effects: Vec<Effect>,
) {
    let mut queue = VecDeque::from(effects);
    let mut steps = 0;
    while let Some(effect) = queue.pop_front() {
        steps += 1;
        assert!(steps <= 128, "bounded voice effects");
        let next = match effect {
            Effect::Load {
                token,
                key,
                legacy,
                session,
            } => {
                // As the driver does: a draft under the name key moves once first.
                if !legacy.is_empty() {
                    store.adopt(&legacy, &key, &session, |_| {});
                }
                composer.loaded(token, Some(load(store, &key)))
            }
            Effect::Save { token, key, draft } => {
                let (tx, rx) = mpsc::channel();
                store.save_durable(&key, draft, move |v| {
                    tx.send(v).unwrap();
                });
                assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().is_ok());
                if token == 0 {
                    Vec::new()
                } else {
                    composer.saved(token, true)
                }
            }
            Effect::Call { token, call } => {
                let Call::Speech { payload } = &call else {
                    panic!("native input forbidden")
                };
                audit(calls, payload);
                let answer = voice_composer::run_call(hgs, &call, known);
                composer.answered(token, answer)
            }
            Effect::Insert { .. } => panic!("historical capture must not insert"),
            Effect::Message { .. } => panic!("native message forbidden"),
            // The parent delivers no timer after killing a caller. Its new
            // caller explicitly checks the original receipt instead.
            _ => Vec::new(),
        };
        queue.extend(next);
    }
}

fn voice(config: &Value, root: &Path, hgs: &Path, calls: &mut Vec<Value>) -> Value {
    let store = DraftStore::open(root.join("drafts"));
    let mut composer = VoiceComposer::new(|| uuid::Uuid::new_v4().to_string());
    let mut known = HashSet::new();
    let target = Target {
        key: "draft\nvoice-recovery".into(),
        session: "fixture/voice".into(),
        run: "fixture-run".into(),
        conversation: "fixture-conversation".into(),
        draft_only: true,
        draft_revision: Some(7),
        available: true,
        ..Target::default()
    };
    let preferences = Preferences {
        machine_id: config["machine"].as_str().unwrap().into(),
        profile_id: "remote".into(),
        ..Preferences::default()
    };
    let effects = composer.update(target.clone(), true, preferences.clone());
    voice_step(&mut composer, &store, hgs, &mut known, calls, effects);
    if config["fresh"] == true {
        let effects = composer.press();
        voice_step(&mut composer, &store, hgs, &mut known, calls, effects);
        let effects = composer.started();
        voice_step(&mut composer, &store, hgs, &mut known, calls, effects);
        let effects = composer.stop_recording(false);
        voice_step(&mut composer, &store, hgs, &mut known, calls, effects);
        let effects = composer.recorded(vec![1; 3200], None);
        voice_step(&mut composer, &store, hgs, &mut known, calls, effects);
    } else {
        // Revoke the current editor before checking. A lookup is allowed to
        // preserve evidence; it never renews insertion or execution permission.
        let revoked = Target {
            available: false,
            ..target
        };
        let effects = composer.update(revoked, false, preferences);
        voice_step(&mut composer, &store, hgs, &mut known, calls, effects);
    }
    let before = load(&store, &composer.storage_key()).form_state;
    let effects = composer.send();
    voice_step(&mut composer, &store, hgs, &mut known, calls, effects);
    let checkpoint = load(&store, &composer.storage_key());
    let audio_sha256 = checkpoint
        .draft
        .attachments
        .first()
        .map(|audio| format!("{:x}", Sha256::digest(&audio.bytes)));
    let after = checkpoint.form_state;
    store.flush();
    json!({"before":before,"after":after,"audio_sha256":audio_sha256,"state":composer.state(),"calls":calls})
}

fn provider_step(
    providers: &mut Providers,
    settings: &Settings,
    hgs: &Path,
    known: &mut HashSet<String>,
    calls: &mut Vec<Value>,
    effects: Vec<ProviderEffect>,
    hold_change_reply: bool,
) {
    let mut queue = VecDeque::from(effects);
    let mut steps = 0;
    while let Some(effect) = queue.pop_front() {
        steps += 1;
        assert!(steps <= 128, "bounded provider effects");
        let next = match effect {
            ProviderEffect::Call(call) => {
                audit(calls, &call.payload);
                let answer = speech_settings::run(hgs, &call.host, &call.payload, known);
                // The original CLI has exited and its resources drained, but
                // the caller can die before consuming this mutation reply.
                if hold_change_reply && call.id == CallId::Change {
                    Vec::new()
                } else {
                    providers.answered(call.id, answer, Instant::now())
                }
            }
            ProviderEffect::StorePending(body) => {
                assert!(speech_settings::keep_pending(settings, &body));
                assert!(speech_settings::pending_written(settings));
                providers.stored(true, Instant::now())
            }
            ProviderEffect::ClearPending => {
                speech_settings::clear_pending(settings);
                Vec::new()
            }
            ProviderEffect::Set(values) => {
                for (key, value) in values {
                    settings.set(key, value).unwrap();
                }
                Vec::new()
            }
            ProviderEffect::PollLater(_) => Vec::new(),
        };
        queue.extend(next);
    }
}

fn provider(config: &Value, root: &Path, hgs: &Path, calls: &mut Vec<Value>) -> Value {
    let settings = Settings::open(root.join("settings.json"));
    let original = settings.text(PENDING_KEY);
    let platform = if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Linux
    };
    let mut providers = Providers::new(platform, "", Defaults::default(), &original, || {
        uuid::Uuid::new_v4().to_string()
    });
    let mut known = HashSet::new();
    let effects = providers.refresh();
    provider_step(
        &mut providers,
        &settings,
        hgs,
        &mut known,
        calls,
        effects,
        false,
    );
    let fresh = config["fresh"] == true;
    let effects = if fresh {
        match config["action"].as_str().unwrap_or("profile_test") {
            "profile_upsert" => {
                assert!(providers.open_editor(false));
                let editor = providers.view().editor.unwrap();
                let start = editor.start;
                providers.save(
                    editor.ticket,
                    EditorForm {
                        name: "Edited recovery fixture".into(),
                        id: start.id,
                        adapter: start.adapter,
                        model: start.model,
                        language: start.language,
                        device: start.device,
                        base_url: start.base_url,
                        allow_insecure_http: start.allow_insecure_http,
                        credential: "keep".into(),
                        key: String::new(),
                    },
                )
            }
            "profile_remove" => {
                let ask = providers.ask_remove().unwrap();
                providers.remove(ask.ticket, true)
            }
            "profile_test" => providers.test(),
            _ => panic!("unknown provider intent"),
        }
    } else {
        providers.check(Instant::now())
    };
    provider_step(
        &mut providers,
        &settings,
        hgs,
        &mut known,
        calls,
        effects,
        fresh,
    );
    assert!(settings.flush_within(Duration::from_secs(5)));
    let after = settings.text(PENDING_KEY);
    json!({"before":serde_json::from_str::<Value>(&original).unwrap_or(Value::Null),
        "after":serde_json::from_str::<Value>(&after).unwrap_or(Value::Null),
        "pending_visible":!providers.view().pending.is_empty(),"calls":calls})
}

#[test]
#[ignore = "explicit exact-child recovery caller; never starts another test"]
fn client_fixture() {
    assert_eq!(
        std::env::var("ZERUS_TEST_SPEECH_RECOVERY").as_deref(),
        Ok("1")
    );
    let control = PathBuf::from(std::env::var_os("ZERUS_TEST_SPEECH_CONTROL").unwrap());
    let bytes = std::fs::read(control).unwrap();
    assert!(bytes.len() <= 4096);
    let config: Value = serde_json::from_slice(&bytes).unwrap();
    let root = Path::new(config["client"].as_str().unwrap());
    let hgs = Path::new(config["hgs"].as_str().unwrap());
    let mut calls = Vec::new();
    let result = match config["kind"].as_str().unwrap() {
        "voice" => voice(&config, root, hgs, &mut calls),
        "provider" => provider(&config, root, hgs, &mut calls),
        _ => panic!("unknown fixture kind"),
    };
    let encoded = serde_json::to_vec(&result).unwrap();
    assert!(encoded.len() <= 65536);
    let destination = Path::new(config["result"].as_str().unwrap());
    let temporary = destination.with_extension("tmp");
    std::fs::write(&temporary, encoded).unwrap();
    std::fs::rename(temporary, destination).unwrap();
    if config["fresh"] == true {
        // The real callers have completed their bounded original CLI calls;
        // the private original checkpoint is durable. The parent SIGKILLs
        // this process before it can receive or persist a later job result.
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("parent did not end its original client fixture");
    }
}
