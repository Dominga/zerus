use super::*;

fn startup(question: &str) -> Value {
    let mut raw = row();
    raw["conversation_id"] = Value::Null;
    raw["reply_id"] = Value::Null;
    raw["phase"] = json!("input");
    raw["activity"] = json!("busy");
    raw["attention_id"] = json!(question);
    raw
}
fn connected() -> Fixture {
    let mut f = Fixture::new();
    f.runtime.set_policy(enabled());
    f.snapshot(vec![]);
    f.ticks(4);
    f
}
fn delivered(f: &mut Fixture) -> String {
    let post = f
        .host
        .try_next()
        .expect("one startup post")
        .begin()
        .unwrap();
    assert!(matches!(post.command().action, Action::Post { .. }));
    let token = post.command().action.token().to_owned();
    post.finish(Outcome::Delivered {
        native_id: NativeId::Macos {
            request_id: attention::request_id(&token),
        },
    });
    f.outbox.poll(&mut f.store, Instant::now());
    token
}
fn withdrawn(f: &mut Fixture, token: &str) {
    f.ticks(8);
    let withdrawal = f
        .host
        .try_next()
        .expect("original withdrawal")
        .begin()
        .unwrap();
    assert!(
        matches!(&withdrawal.command().action, Action::Withdraw { token: original, .. } if original == token)
    );
    withdrawal.finish(Outcome::Delivered {
        native_id: NativeId::Macos {
            request_id: attention::request_id(token),
        },
    });
    f.outbox.poll(&mut f.store, Instant::now());
    f.ticks(8);
    assert!(f.host.try_next().is_err(), "no replacement or replay");
}

#[test]
fn mandatory_startup_questions_post_once_and_answer_withdraws_the_original() {
    for question in [
        "codex-trust:one",
        "codex-hooks-trust:one",
        "codex-update:one",
        "claude-trust:one",
        "claude-permissions:one",
        "kimi-trust:one",
        "codex-goal-resume:one",
    ] {
        let mut f = connected();
        let mut raw = startup(question);
        f.delta(raw.clone());
        f.ticks(8);
        let token = delivered(&mut f);
        let target = attention::target(&token).unwrap();
        assert_eq!(target["startup_question"], question);
        assert_eq!(target["run"], "run");
        assert_eq!(target["conversation"], "");
        raw["name"] = json!("codex/renamed/startup");
        f.delta(raw.clone());
        f.ticks(8);
        assert!(f.host.try_next().is_err(), "rename does not post again");
        assert_eq!(f.intents(), 1);
        raw["phase"] = json!("idle");
        raw["activity"] = json!("idle");
        raw["attention_id"] = Value::Null;
        f.delta(raw);
        withdrawn(&mut f, &token);
    }
}

#[test]
fn parallel_unbound_runs_and_new_questions_have_separate_native_identities() {
    let mut f = connected();
    let first = startup("codex-trust:same");
    let mut second = first.clone();
    second["name"] = json!("codex/example/second");
    second["run_id"] = json!("second-run");
    f.delta(first.clone());
    f.delta(second.clone());
    f.ticks(8);
    let a = delivered(&mut f);
    let b = delivered(&mut f);
    assert_ne!(a, b);
    assert_eq!(f.intents(), 2);
    let mut changed = first;
    changed["attention_id"] = json!("codex-hooks-trust:new");
    f.delta(changed);
    f.ticks(8);
    let mut posts = 0;
    let mut withdrawals = 0;
    while let Ok(next) = f.host.try_next() {
        let op = next.begin().unwrap();
        match &op.command().action {
            Action::Post { token, .. } => {
                assert_eq!(
                    attention::target(token).unwrap()["startup_question"],
                    "codex-hooks-trust:new"
                );
                posts += 1;
            }
            Action::Withdraw { token, .. } => {
                assert_eq!(attention::target(token).unwrap()["run"], "run");
                withdrawals += 1;
            }
        }
        let token = op.command().action.token().to_owned();
        op.finish(Outcome::Delivered {
            native_id: NativeId::Macos {
                request_id: attention::request_id(&token),
            },
        });
        f.outbox.poll(&mut f.store, Instant::now());
    }
    assert_eq!((posts, withdrawals), (1, 1));
}

#[test]
fn conversation_handover_withdraws_startup_without_reposting_the_same_question() {
    for still_asking in [false, true] {
        let mut f = connected();
        let mut raw = startup("claude-permissions:original");
        f.delta(raw.clone());
        f.ticks(8);
        let token = delivered(&mut f);
        raw["conversation_id"] = json!("first-conversation");
        if !still_asking {
            raw["phase"] = json!("idle");
            raw["activity"] = json!("idle");
            raw["attention_id"] = Value::Null;
        }
        f.delta(raw.clone());
        withdrawn(&mut f, &token);
        // A new service/source baseline preserves the durable consumption.
        f.runtime = Runtime::new(&f.source.service_epoch);
        f.runtime.clock = Some(Instant::now());
        f.runtime.set_policy(enabled());
        f.source.incarnation += 1;
        f.seq = 0;
        f.snapshot(vec![raw.clone()]);
        f.ticks(8);
        f.delta(raw.clone());
        f.ticks(8);
        assert!(f.host.try_next().is_err());
        raw["attention_id"] = json!("claude-permissions:different");
        raw["phase"] = json!("input");
        raw["activity"] = json!("busy");
        f.delta(raw);
        f.ticks(8);
        delivered(&mut f);
    }
}

#[test]
fn an_answered_startup_question_cannot_become_fresh_after_another_conversation_question() {
    let mut f = connected();
    let mut raw = startup("codex-trust:original");
    f.delta(raw.clone());
    f.ticks(8);
    let original = delivered(&mut f);
    raw["phase"] = json!("idle");
    raw["activity"] = json!("idle");
    raw["attention_id"] = Value::Null;
    f.delta(raw.clone());
    withdrawn(&mut f, &original);
    raw["conversation_id"] = json!("bound");
    raw["phase"] = json!("input");
    raw["activity"] = json!("busy");
    raw["attention_id"] = json!("different-question");
    f.delta(raw.clone());
    f.ticks(8);
    let different = delivered(&mut f);
    raw["attention_id"] = json!("codex-trust:original");
    f.delta(raw);
    withdrawn(&mut f, &different);
}

#[test]
fn startup_reconnect_and_an_unknown_native_outcome_never_replay() {
    for unknown in [false, true] {
        let mut f = connected();
        let raw = startup("kimi-trust:original");
        f.delta(raw.clone());
        f.ticks(8);
        let op = f.host.try_next().unwrap().begin().unwrap();
        let token = op.command().action.token().to_owned();
        op.finish(if unknown {
            Outcome::Unknown {
                code: zerus_client_core::desktop::notifications::Uncertainty::Transport,
            }
        } else {
            Outcome::Delivered {
                native_id: NativeId::Macos {
                    request_id: attention::request_id(&token),
                },
            }
        });
        f.outbox.poll(&mut f.store, Instant::now());
        f.runtime = Runtime::new(&f.source.service_epoch);
        f.runtime.clock = Some(Instant::now());
        f.runtime.set_policy(enabled());
        f.source.incarnation += 1;
        f.seq = 0;
        f.snapshot(vec![raw.clone()]);
        f.ticks(8);
        f.delta(raw);
        f.ticks(8);
        assert!(f.host.try_next().is_err());
        let attempts: i64 = f
            .store
            .db
            .query_row("SELECT count(*) FROM desktop_attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(attempts, 1);
        assert_eq!(f.intents(), 1);
    }
}

#[test]
fn startup_baseline_focus_and_quiet_children_keep_the_shared_policy() {
    let mut f = Fixture::new();
    f.runtime.set_policy(enabled());
    let raw = startup("codex-trust:old");
    f.snapshot(vec![raw.clone()]);
    f.ticks(8);
    f.delta(raw);
    f.ticks(8);
    assert!(
        f.host.try_next().is_err(),
        "startup baseline is not a new edge"
    );
    let mut parented = startup("codex-trust:human");
    parented["parent"] = json!({"schema":1,"machine":"11111111-1111-4111-8111-111111111111","host":"arch",
        "session_id":"22222222-2222-4222-8222-222222222222","name":"claude/example/lead",
        "conversation_id":"c","run_id":"r","source":"launch","at":1,"state":"live"});
    parented["human_request"] = json!(false);
    // A mandatory native trust prompt is person-only even if an old host
    // supplies a false flag; the shared view's classification must win.
    let mut routine = parented.clone();
    routine["attention_id"] = json!("ordinary-agent-request");
    assert!(project(&routine).candidates.iter().all(|c| c.consumed));
    parented["human_request"] = json!(true);
    assert!(project(&parented).candidates.iter().any(|c| !c.consumed));
    let p =
        projection::project("", "fixture", &parented, Map::new(), enabled(), true, false).unwrap();
    assert!(p.candidates.iter().all(|c| c.consumed));

    let mut f = connected();
    f.delta(routine.clone());
    f.ticks(8);
    assert!(f.host.try_next().is_err());
    assert_eq!(f.intents(), 0, "a quiet child's request is not remembered");
    routine["parent"]["state"] = json!("saved");
    f.delta(routine);
    f.ticks(8);
    delivered(&mut f);
}

#[test]
fn absent_question_identity_cannot_fall_back_to_a_name_clock_reply_or_child() {
    for missing in [
        Value::Null,
        json!(""),
        json!("bad\nidentity"),
        json!("x".repeat(1025)),
    ] {
        let mut raw = startup("placeholder");
        raw["attention_id"] = missing;
        raw["attention_started"] = json!(1234);
        raw["reply_id"] = json!("unexpected-reply");
        raw["subagents"] = json!({"child":{"display_state":"input","attention_id":"child-q"}});
        raw["optional_questions"] = json!({"count":1,"ids":["optional-q"]});
        assert!(project(&raw).candidates.is_empty());
    }
    let mut malformed = startup("question");
    malformed["conversation_id"] = json!(42);
    assert!(projection::project(
        "",
        "fixture",
        &malformed,
        Map::new(),
        enabled(),
        false,
        false
    )
    .is_err());
}

#[test]
fn schema_32_preserves_existing_conversation_keys_and_indexes_the_new_run_lookup() {
    let mut f = Fixture::new();
    f.save(&row(), false);
    let original: (String, String, String) = f
        .store
        .db
        .query_row("SELECT id,identity,payload FROM desktop_intents", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    let location = Location {
        root: std::path::Path::new(f.store.db.path().unwrap())
            .parent()
            .unwrap()
            .to_owned(),
        configuration: "a".repeat(64),
    };
    // The later schema-34 session resume leaf is rewound too.
    crate::desktop::session_resume::remove_fixture(&f.store.db);
    f.store
        .db
        .execute_batch("DROP INDEX desktop_attention_catalog_run; PRAGMA user_version=31;")
        .unwrap();
    drop(f.store);
    let reopened = Store::open(&location, &id()).unwrap();
    let after: (String, String, String) = reopened
        .db
        .query_row("SELECT id,identity,payload FROM desktop_intents", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(after, original);
    assert_eq!(
        reopened
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        crate::desktop::store::SCHEMA
    );
    let mut query = reopened
        .db
        .prepare(&format!("EXPLAIN QUERY PLAN {}", state::STARTUP_CURRENT))
        .unwrap();
    let plan = query
        .query_map(["", "codex", "run"], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(plan.contains("desktop_attention_catalog_run"), "{plan}");
    assert!(!plan.contains("SCAN r"), "{plan}");
}
