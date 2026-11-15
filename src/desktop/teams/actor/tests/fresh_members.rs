//! Product-written fresh sessions, catalog frames, task documents, hand-outs
//! and native receipts. Screens/native metadata are the explicit stand-ins.
use super::super::{
    native::Native,
    ports::{Availability, Fact, Ports},
    Runtime,
};
use crate::{
    state::{fresh_member_fixture::Terminal, listing_fixture as producer},
    teams::ops::tests::*,
};
use serde_json::{json, Value};
use std::{
    process::Command,
    sync::{atomic::AtomicBool, Arc},
};
use zerus_team_plan::model::{Member, TaskRef};
use zerus_watch_protocol::{topic, Body, CatalogItem, Frame, Hello, Limits, Snapshot};

const CHILD: &str = "desktop::teams::actor::tests::fresh_members::fixture";
#[test]
fn first_message_probe_uses_the_production_cli_argument_parser() {
    child("cli-arguments");
}

const RUN: &str = "fresh-member-run";
const SID: &str = "00000000-0000-4000-8000-000000000152";

fn child(case: &str) {
    let root = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(target) => tempfile::tempdir_in(target),
        None => tempfile::tempdir(),
    }
    .unwrap();
    crate::platform::restrict_dir(root.path()).unwrap();
    // Leave room for tmux's suffix and even a ten-digit UID on both Unix hosts.
    // macOS has 104 sun_path bytes (including NUL), Linux has 108.
    let socket = root
        .path()
        .canonicalize()
        .unwrap()
        .join("tmux")
        .join(format!("tmux-{}", u32::MAX))
        .join("default");
    assert!(socket.as_os_str().len() < 104, "tmux socket path: {socket:?}");
    for dir in ["state", "cfg", "tmux"] {
        crate::platform::private_dir(&root.path().join(dir)).unwrap();
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            CHILD,
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("HGS_FRESH_MEMBER_FIXTURE", case)
        .env("HOME", root.path())
        .env("HGS_STATE_DIR", root.path().join("state"))
        .env("HGS_CONFIG_DIR", root.path().join("cfg"))
        .env("TMPDIR", root.path())
        .env("TMUX_TMPDIR", root.path().join("tmux"))
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .env_remove("HGS_LAUNCH_ID")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success() && text.contains(&format!("FRESH_MEMBER_EXECUTED={case}")),
        "{text}"
    );
}

#[test]
fn a_fresh_codex_gets_the_task_as_its_first_native_message() {
    child("codex-ready");
}
#[test]
fn fresh_claude_and_kimi_use_the_native_composer_gate() {
    child("other-agents");
}
#[test]
fn fresh_trust_and_unready_terminals_are_named_after_the_usual_wait() {
    child("unready");
}
#[test]
fn positive_work_and_pending_questions_never_make_a_fresh_member_free() {
    child("positive");
}
#[test]
fn established_conversations_keep_the_known_idle_rule() {
    child("established");
}
#[test]
fn a_first_send_proof_is_exact_and_missing_is_not_ready() {
    child("proof");
}
#[test]
fn only_the_team_bound_launch_gets_the_fresh_exception() {
    child("binding");
}
#[test]
fn first_send_readiness_is_checked_again_before_admission() {
    child("race");
}

#[test]
fn a_held_fresh_member_gets_one_readiness_card_and_no_generic_card() {
    child("held-unready");
}

#[test]
fn final_admission_rejects_a_stale_first_message_witness() {
    child("admission-match");
}
#[test]
fn a_first_message_witness_requires_the_members_agent() {
    child("member-agent");
}
#[test]
fn a_first_message_witness_requires_the_members_launch() {
    child("member-launch");
}
#[test]
fn a_first_message_witness_refuses_an_empty_identity() {
    child("empty-identity");
}
#[test]
fn unsettled_children_keep_a_fresh_member_from_being_free() {
    child("unsettled-children");
}

struct World {
    h: Harness,
    n: Native,
    terminal: &'static Terminal,
    member: Member,
    task: TaskRef,
    row: Value,
    seq: u64,
}
impl World {
    fn new(agent: &str, shape: &str, confirmed: bool) -> Self {
        let mut h = Harness::empty();
        let mut op = create();
        if let crate::teams::ops::Operation::Create { bundle, .. } = &mut op {
            bundle
                .roles
                .iter_mut()
                .find(|r| r.id == "developer")
                .unwrap()
                .agent = agent.into();
            bundle.pipelines[0].stages.clear();
        }
        h.run(&person(), op).unwrap();
        h.bind("lead");
        h.bind("m1");
        crate::platform::restrict_dir(h.temp.path()).unwrap();
        std::env::set_var("HGS_STATE_DIR", h.temp.path());
        let team = h.store.team(TEAM).unwrap().unwrap();
        let member = h.store.member(TEAM, "m1").unwrap().unwrap();
        assert_eq!(member.agent, agent);
        let config = crate::config::Config {
            home: String::new(),
            dir: h.temp.path().join("config"),
            state: h.temp.path().to_owned(),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        crate::platform::private_dir(&config.dir).unwrap();
        crate::platform::private_dir(&config.state.join("sessions")).unwrap();
        crate::swarm::fixture_project(&config, &team.home, &team.project);
        let n = Native {
            config,
            machine: team.home.clone(),
            epoch: "fresh-fixture".into(),
            stop: Arc::new(AtomicBool::new(false)),
        };
        super::super::native::TEST_INVENTORY.with(|s| {
            *s.borrow_mut() = Some(json!({"node_id":team.home,"organization":{"projects":[]}}))
        });
        let launch = member
            .session
            .as_ref()
            .unwrap()
            .session_id
            .strip_prefix("launch_id:")
            .unwrap();
        let name = format!("{agent}/fresh/member");
        let mut record = producer::launched_record(
            agent,
            &name,
            launch,
            SID,
            RUN,
            &member.worktree,
            &member.account,
        );
        if confirmed {
            producer::confirmed(&mut record, "existing-conversation");
        }
        let lead = h.store.member(TEAM, "lead").unwrap().unwrap();
        let lead_launch = lead
            .session
            .as_ref()
            .unwrap()
            .session_id
            .strip_prefix("launch_id:")
            .unwrap();
        let lead_name = format!("{}/fresh/lead", lead.agent);
        let mut lead_record = producer::launched_record(
            &lead.agent,
            &lead_name,
            lead_launch,
            "00000000-0000-4000-8000-000000000153",
            "fixture-lead-run",
            &lead.worktree,
            &lead.account,
        );
        producer::confirmed(&mut lead_record, "fixture-lead-conversation");
        producer::save(&mut lead_record).unwrap();
        producer::launch_started(
            lead_launch,
            &lead_name,
            "00000000-0000-4000-8000-000000000153",
        )
        .unwrap();
        let terminal = Terminal::new(record, shape);
        producer::launch_started(launch, &name, SID).unwrap();
        let (row, task) = terminal.with(|| {
            producer::complete_questions(&terminal.record).unwrap();
            let row = producer::listed_live_row(terminal.record.clone()).unwrap();
            let store =
                zerus_replica::ProjectStore::open(&n.config.state.join("replica"), &team.project)
                    .unwrap();
            let (mut doc, _) = store
                .new_named_task(
                    "First task",
                    "Implement the fresh-member fixture task",
                    &[],
                    zerus_replica::Stage::Todo,
                    zerus_replica::Target::Any,
                    zerus_replica::Actor::Person,
                    &zerus_replica::Position::End,
                    "example",
                    1,
                    None,
                    false,
                )
                .unwrap();
            let task = TaskRef {
                project: team.project.clone(),
                uuid: doc.uuid().into(),
            };
            store.save(&mut doc).unwrap();
            drop(store);
            crate::teams::tasks::send(
                &crate::teams::tasks::Caller {
                    state: &n.config.state,
                    machine: &team.home,
                    actor: zerus_replica::Actor::Person,
                },
                &task,
                TEAM,
                Some("developer"),
                Some("reviewed"),
            )
            .unwrap();
            (row, task)
        });
        let mut w = Self {
            h,
            n,
            terminal,
            member,
            task,
            row,
            seq: 1,
        };
        w.publish(true);
        w
    }
    fn publish(&mut self, first: bool) {
        let directory = self.n.config.state.join("desktop");
        crate::platform::private_dir(&directory).unwrap();
        let path = directory.join("state.sqlite3");
        if first {
            crate::sqlite_file::create(&path).unwrap();
        }
        let mut db = rusqlite::Connection::open(path).unwrap();
        if first {
            zerus_desktop_catalog::migrate(&db).unwrap();
        }
        let source = zerus_desktop_catalog::Source {
            computer: String::new(),
            service_epoch: self.n.epoch.clone(),
            incarnation: 1,
            generation: 1,
        };
        if first {
            zerus_desktop_catalog::begin_service(&mut db, &self.n.epoch).unwrap();
            zerus_desktop_catalog::connect(
                &mut db,
                &source,
                &Frame {
                    v: 1,
                    seq: 1,
                    body: Body::Hello(Hello {
                        machine_id: Some(self.n.machine.clone()),
                        host: "fixture".into(),
                        hgs_version: "1.0.0".into(),
                        stream: "fresh".into(),
                        heartbeat_ms: zerus_watch_protocol::HEARTBEAT_MS,
                        max_frame_bytes: zerus_watch_protocol::MAX_FRAME_BYTES,
                        topics: vec![topic::CATALOG.into()],
                        limits: Limits::default(),
                    }),
                },
            )
            .unwrap();
        }
        self.seq += 1;
        zerus_desktop_catalog::apply(
            &mut db,
            &source,
            &Frame {
                v: 1,
                seq: self.seq,
                body: Body::Snapshot(Snapshot {
                    topic: topic::CATALOG.into(),
                    key: None,
                    value: None,
                    items: Some(vec![serde_json::to_value(CatalogItem {
                        key: format!("run:{RUN}"),
                        row: self.row.clone(),
                    })
                    .unwrap()]),
                    page: 0,
                    last: true,
                    cursor: self.seq.try_into().unwrap(),
                }),
            },
        )
        .unwrap();
    }
    fn transport(&self, fault: Option<(&'static str, Value)>) {
        let terminal = self.terminal;
        crate::desktop::teams::child::FIXTURE_SPAWN.with(|s| {
            *s.borrow_mut() = Some(Box::new(move |config, args, input| {
                let value = if args.first().is_some_and(|a| a == "inspect") {
                    let mut value = match terminal.cli_inspection(args) {
                        Ok(value) => value,
                        Err(error) => {
                            println!("INSPECT_ARGUMENTS_REFUSED={args:?}: {error}");
                            return crate::desktop::teams::child::Ended::Exited {
                                code: 2,
                                output: error.into_bytes(),
                            };
                        }
                    };
                    if let Some((key, value_fault)) = &fault {
                        if *key == "after-reservation" {
                            let store = crate::teams::store::Store::open(&config.state).unwrap();
                            let reserved = store
                                .effects(TEAM, 200, None)
                                .unwrap()
                                .rows
                                .iter()
                                .any(|e| e.kind == "handout" && e.state == "reserved");
                            if reserved {
                                value["first_message_can_send"] = value_fault.clone();
                            }
                        } else {
                            value[*key] = value_fault.clone();
                        }
                    }
                    value
                } else {
                    assert_eq!(&args[..2], ["task", "team-deliver"]);
                    crate::tasks::fixture_delivery(config, &args[2..], input.unwrap())
                };
                crate::desktop::teams::child::Ended::Exited {
                    code: 0,
                    output: value.to_string().into_bytes(),
                }
            }))
        });
    }
    fn availability(&mut self) -> Availability {
        let team = self.h.store.team(TEAM).unwrap().unwrap();
        Ports::availability(&mut self.n, &team, &self.member).unwrap()
    }
    fn pass(&mut self, runtime: &mut Runtime, now: i64) {
        runtime.begin_pass();
        runtime
            .tick(&mut self.h.store, &mut self.n, TEAM, now, false)
            .unwrap();
    }
    fn cards(&self) -> Vec<String> {
        self.h
            .store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .filter(|c| c.state == "open")
            .map(|c| c.text)
            .collect()
    }
}
impl Drop for World {
    fn drop(&mut self) {
        crate::desktop::teams::child::FIXTURE_SPAWN.with(|s| s.borrow_mut().take());
        super::super::native::TEST_INVENTORY.with(|s| s.borrow_mut().take());
    }
}

#[test]
#[ignore = "executed by the isolated controls"]
fn fixture() {
    let case = std::env::var("HGS_FRESH_MEMBER_FIXTURE").unwrap();
    match case.as_str() {
        "cli-arguments" => {
            let mut w = World::new("codex", "ready", false);
            w.terminal.with(|| {
                w.transport(None);
                assert_eq!(w.availability(), Availability::Free);
            });
        }

        "codex-ready" => {
            let mut w = World::new("codex", "ready", false);
            let terminal = w.terminal;
            terminal.with(|| {
                assert!(w.row["conversation_id"].is_null());
                assert_eq!(w.row["activity"], "unknown");
                assert!(w.row["process_work"]["count"].is_null());
                w.transport(None);
                assert_eq!(w.availability(), Availability::Free);
                let mut runtime = Runtime::open(w.h.temp.path(), &w.n.machine).unwrap();
                w.pass(&mut runtime, 10_000_000);
                let effects = w.h.store.effects(TEAM, 200, None).unwrap().rows;
                assert!(
                    effects
                        .iter()
                        .any(|e| e.kind == "handout" && e.state == "submitted"),
                    "{effects:?}"
                );
                let messages = terminal.submissions.lock().unwrap();
                assert_eq!(messages.len(), 1);
                assert!(messages[0].contains("Implement the fresh-member fixture task"));
                drop(messages);
                let tasks = zerus_replica::ProjectStore::open(
                    &w.n.config.state.join("replica"),
                    &w.task.project,
                )
                .unwrap();
                let doc = tasks.load(&w.task.uuid).unwrap();
                let effect = effects.iter().find(|e| e.kind == "handout").unwrap();
                let attempt = doc.attempt(&effect.request_id).unwrap().unwrap();
                let native = attempt.delivery.unwrap();
                assert!(native.conversation_id.is_empty());
                drop(tasks);
                let receipt = crate::state::task_reply_receipt_bound(
                    w.terminal.record["name"].as_str().unwrap(),
                    &effect.request_id,
                    &native.digest,
                )
                .unwrap();
                let crate::state::ReceiptLookup::Found(receipt) = receipt else {
                    panic!("missing native receipt")
                };
                assert_eq!(receipt["first_message"], true);
                assert_eq!(receipt["status"], "submitted");
                w.pass(&mut runtime, 10_001_000);
                assert_eq!(
                    terminal.submissions.lock().unwrap().len(),
                    1,
                    "never replay a first message"
                );
            });
        }
        "other-agents" => {
            for agent in ["claude", "kimi"] {
                let mut w = World::new(agent, "ready", false);
                w.terminal.with(|| {
                    w.transport(None);
                    assert_eq!(w.availability(), Availability::Free, "{agent}");
                });
            }
        }
        "unready" => {
            for shape in ["trust", "starting"] {
                let mut w = World::new("codex", shape, false);
                w.terminal.with(|| {
                    w.transport(None);
                    assert_eq!(
                        w.availability(),
                        Availability::Unreadable(Fact::FirstMessage)
                    );
                    let mut a = Runtime::open(w.h.temp.path(), &w.n.machine).unwrap();
                    for i in 0..=3 {
                        w.pass(&mut a, 10_000_000 + i * 60_000);
                    }
                    let cards = w.cards();
                    assert!(
                        cards.iter().any(|c| c.contains("Member m1")
                            && c.contains("waiting for its first-message readiness")),
                        "{cards:?}"
                    );
                    assert!(w.terminal.submissions.lock().unwrap().is_empty());
                });
            }
        }
        "positive" => {
            for path in [
                "process_work",
                "subagent_active_count",
                "optional_questions",
            ] {
                let mut w = World::new("codex", "ready", false);
                match path {
                    "process_work" => w.row[path]["count"] = json!(1),
                    "optional_questions" => w.row[path]["count"] = json!(1),
                    _ => w.row[path] = json!(1),
                };
                w.publish(false);
                w.terminal.with(|| {
                    w.transport(None);
                    assert_ne!(w.availability(), Availability::Free, "{path}");
                });
            }
        }
        "established" => {
            let mut w = World::new("codex", "starting", true);
            w.terminal.with(|| {
                w.transport(Some(("first_message_can_send", json!(true))));
                assert_eq!(w.availability(), Availability::Free);
                w.row["process_work"]["count"] = Value::Null;
                w.publish(false);
                assert_eq!(w.availability(), Availability::Unreadable(Fact::Background));
            });
        }
        "proof" => {
            for (key, value) in [
                ("first_message_can_send", Value::Null),
                ("first_message_can_send", json!(false)),
                ("run_id", json!("other")),
                ("launch_id", json!("other")),
                ("conversation_id", json!("new-conversation")),
                ("name", json!("codex/other/member")),
                ("agent", json!("kimi")),
                ("process_work", json!({"count":1})),
                ("subagent_active_count", json!(1)),
            ] {
                let mut w = World::new("codex", "ready", false);
                w.terminal.with(|| {
                    w.transport(Some((key, value)));
                    assert_eq!(
                        w.availability(),
                        Availability::Unreadable(Fact::FirstMessage),
                        "{key}"
                    );
                });
            }
        }
        "binding" => {
            let mut w = World::new("codex", "ready", false);
            w.member.session.as_mut().unwrap().session_id = format!("run_id:{RUN}");
            w.terminal.with(|| {
                w.transport(None);
                assert_ne!(w.availability(), Availability::Free);
            });
        }
        "race" => {
            let mut w = World::new("codex", "ready", false);
            w.terminal.with(|| {
                w.transport(None);
                assert_eq!(w.availability(), Availability::Free);
                w.transport(Some(("after-reservation", json!(false))));
                let mut runtime = Runtime::open(w.h.temp.path(), &w.n.machine).unwrap();
                w.pass(&mut runtime, 10_000_000);
                let effects = w.h.store.effects(TEAM, 200, None).unwrap().rows;
                assert!(effects.iter().any(|e| e.kind == "handout"));
                assert!(effects.iter().all(|e| e.state != "submitted"));
                let tasks = zerus_replica::ProjectStore::open(
                    &w.n.config.state.join("replica"),
                    &w.task.project,
                )
                .unwrap();
                let doc = tasks.load(&w.task.uuid).unwrap();
                for effect in effects.iter().filter(|e| e.kind == "handout") {
                    assert!(doc.attempt(&effect.request_id).unwrap().is_none());
                }
                assert!(w.terminal.submissions.lock().unwrap().is_empty());
            });
        }
        "held-unready" => {
            let mut w = World::new("codex", "ready", false);
            w.terminal.with(|| {
                w.transport(Some(("after-reservation", json!(false))));
                let mut runtime = Runtime::open(w.h.temp.path(), &w.n.machine).unwrap();
                for i in 0..=3 {
                    w.pass(&mut runtime, 10_000_000 + i * 60_000);
                    println!("HELD_READINESS_CARDS[{i}]={:?}", w.cards());
                }
                let cards = w.cards();
                assert_eq!(
                    cards.len(),
                    1,
                    "one named card, no generic handout card: {cards:?}"
                );
                assert!(
                    cards[0].contains("Member m1")
                        && cards[0].contains("waiting for its first-message readiness"),
                    "{cards:?}"
                );
                assert!(!cards[0].contains("Delivery did not start"), "{cards:?}");
                assert!(w.terminal.submissions.lock().unwrap().is_empty());
                w.pass(&mut runtime, 10_240_000);
                assert_eq!(w.cards(), cards, "the named card is deduplicated");
                assert_eq!(w.h.store.cards(TEAM, 200, None).unwrap().rows.len(), 1);
                w.transport(None);
                w.pass(&mut runtime, 10_300_000);
                assert!(w.cards().is_empty(), "readiness recovery closes the card");
                let history = w.h.store.cards(TEAM, 200, None).unwrap().rows;
                assert_eq!(history.len(), 1);
                assert_ne!(history[0].state, "open");
                assert_eq!(w.terminal.submissions.lock().unwrap().len(), 1);
            });
        }
        "admission-match" => {
            let mut w = World::new("codex", "ready", false);
            w.terminal.with(|| {
                // Reserve through the actual actor, without offering input.
                w.transport(Some(("after-reservation", json!(false))));
                let mut runtime = Runtime::open(w.h.temp.path(), &w.n.machine).unwrap();
                w.pass(&mut runtime, 10_000_000);
                let effect =
                    w.h.store
                        .effects(TEAM, 200, None)
                        .unwrap()
                        .rows
                        .into_iter()
                        .find(|e| e.kind == "handout" && e.state == "reserved")
                        .expect("the actor reserved a hand-out");
                let team = w.h.store.team(TEAM).unwrap().unwrap();
                let member = w.member.clone();
                let node = w.h.store.node(TEAM, effect.node).unwrap().unwrap();
                let row = w.row.clone();
                let n = Native {
                    config: w.n.config.clone(),
                    machine: w.n.machine.clone(),
                    epoch: w.n.epoch.clone(),
                    stop: Arc::clone(&w.n.stop),
                };
                w.transport(None);
                let target = (&team, &node, &member, effect.request_id.as_str(), &row);
                n.fixture_first_admission(target, || {}).unwrap();
                assert!(
                    n.fixture_first_admission(target, || {
                        // A catalog change after the proof, before the real final
                        // admission. The binding/run stay readable; the proof's
                        // exact name no longer matches the current producer row.
                        w.row["name"] = json!("codex/fresh/renamed");
                        w.publish(false);
                    })
                    .is_err(),
                    "a stale proof cannot admit the reserved child"
                );
                assert!(w.terminal.submissions.lock().unwrap().is_empty());
            });
        }
        "member-agent" => {
            let w = World::new("codex", "ready", false);
            w.terminal.with(|| {
                w.transport(None);
                assert!(w.n.fixture_first_ready(&w.member, &w.row));
                let mut member = w.member.clone();
                member.agent = "kimi".into();
                assert!(!w.n.fixture_first_ready(&member, &w.row));
            });
        }
        "member-launch" => {
            let w = World::new("codex", "ready", false);
            w.terminal.with(|| {
                w.transport(None);
                assert!(w.n.fixture_first_ready(&w.member, &w.row));
                let mut row = w.row.clone();
                row["launch_id"] = json!("another-launch");
                w.transport(Some(("launch_id", json!("another-launch"))));
                assert!(!w.n.fixture_first_ready(&w.member, &row));
            });
        }
        "empty-identity" => {
            for key in ["name", "run_id", "launch_id", "agent"] {
                let w = World::new("codex", "ready", false);
                w.terminal.with(|| {
                    w.transport(None);
                    assert!(w.n.fixture_first_ready(&w.member, &w.row));
                    let mut row = w.row.clone();
                    row[key] = json!("");
                    w.transport(Some((key, json!(""))));
                    assert!(!w.n.fixture_first_ready(&w.member, &row), "empty {key}");
                });
            }
        }
        "unsettled-children" => {
            let mut w = World::new("codex", "ready", false);
            w.terminal.with(|| {
                w.transport(None);
                assert_eq!(w.availability(), Availability::Free);
                w.row["subagent_active_count"] = json!(0);
                w.row["subagent_source"] = json!("hook_profiles");
                w.row["subagent_groups_complete"] = json!(false);
                w.publish(false);
                assert!(super::super::fleet::Facts::of(&w.row).children_unsettled);
                assert_eq!(
                    w.availability(),
                    Availability::Unreadable(Fact::FirstMessage)
                );
            });
        }
        _ => panic!("unknown fresh-member case"),
    }
    println!("FRESH_MEMBER_EXECUTED={case}");
}
