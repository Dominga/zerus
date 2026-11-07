use super::*;
use crate::desktop::{feed, store::Location};
use rusqlite::params;
use std::sync::{atomic::AtomicUsize, Mutex};
use zerus_watch_protocol::CatalogItem;

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[test]
fn committed_policy_receipt_remains_readable_without_a_runtime() {
    let mut f = Fixture::new(Arc::new(|_, _, _, _| panic!("no executor needed")));
    f.active();
    let receipt = f.enabled(true);
    let location = f.location;
    drop(f.runtime);
    drop(f.store);
    let store = Store::open(&location, &id()).unwrap();
    let response = result_request(
        &store,
        &json!({"op":"delivery_result",
        "request_id":id(),"original_request_id":receipt["request_id"]}),
    );
    assert_eq!(response["result"], receipt);
    assert_eq!(response["found"], true);
    assert_eq!(response["durable"], false);
    assert_eq!(
        result_request(
            &store,
            &json!({"op":"automatic_approvals",
        "request_id":id(),"enabled":false})
        )["accepted"],
        false
    );
    assert!(state::policy(&store.db).unwrap().enabled);
}
struct Fixture {
    _temp: tempfile::TempDir,
    location: Location,
    store: Store,
    runtime: Runtime,
    feed: feed::Broker,
    producer: Arc<feed::Producer>,
    generation: u64,
}
impl Fixture {
    fn passive_projection(&self) -> Value {
        crate::desktop::projection(
            &self.location,
            &self.store,
            &self.runtime.epoch,
            0,
            &Value::Null,
            &self.feed,
            &crate::desktop::catalog_read::Reader::default(),
        )
    }
    fn projection(&self) -> Value {
        let mut value = self.passive_projection();
        self.runtime.decorate(&mut value);
        value
    }
    fn new(run: Runner) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: temp.path().canonicalize().unwrap().join("state/desktop"),
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        let epoch = id();
        let store = Store::open(&location, &epoch).unwrap();
        let mut feed = feed::Broker::new(&epoch);
        let producer = feed.claim_internal().unwrap();
        let runtime = Runtime::new(
            &epoch,
            temp.path().canonicalize().unwrap().join("approval-audit"),
            run,
        );
        Self {
            _temp: temp,
            location,
            store,
            runtime,
            feed,
            producer,
            generation: 0,
        }
    }
    fn active(&mut self) {
        zerus_desktop_ledger::authorize(&mut self.store.db, &id(), &self.runtime.epoch).unwrap();
        self.runtime.poll(&mut self.store);
    }
    fn enabled(&mut self, enabled: bool) -> Value {
        self.native_policy(&json!({"op":"automatic_approvals","request_id":id(),"enabled":enabled}))
    }
    fn native_policy(&mut self, value: &Value) -> Value {
        self.runtime.frontend_request(
            &mut self.store,
            crate::desktop::ui_authority::Validated::fixture(value),
        )
    }
    fn source(&mut self, row: Value, native: bool) {
        self.generation += 1;
        for packet in [
            feed::Packet::LegacyBegin {
                computer: String::new(),
                generation: self.generation,
                metadata: json!({"machine_id":"fixture"}),
                rows: 1,
                pages: 1,
            },
            feed::Packet::LegacyPage {
                computer: String::new(),
                generation: self.generation,
                index: 0,
                items: vec![CatalogItem {
                    key: "run:run".into(),
                    row,
                }],
            },
            feed::Packet::LegacyComplete {
                computer: String::new(),
                generation: self.generation,
            },
        ] {
            self.feed
                .apply(&mut self.store, &self.producer, &packet)
                .unwrap();
        }
        if native {
            self.runtime.sources.native_fixture(&self.store, "");
        }
    }
    /// A native source of many rows, in legacy pages of the largest size.
    fn source_rows(&mut self, rows: Vec<CatalogItem>) {
        self.source_rows_on("", rows);
    }
    /// `source_rows` for the computer `computer` ("" is this one).
    fn source_rows_on(&mut self, computer: &str, rows: Vec<CatalogItem>) {
        self.generation += 1;
        let pages: Vec<Vec<CatalogItem>> = rows.chunks(128).map(<[_]>::to_vec).collect();
        let mut packets = vec![feed::Packet::LegacyBegin {
            computer: computer.into(),
            generation: self.generation,
            metadata: json!({"machine_id":"fixture"}),
            rows: rows.len(),
            pages: pages.len(),
        }];
        for (index, items) in pages.into_iter().enumerate() {
            packets.push(feed::Packet::LegacyPage {
                computer: computer.into(),
                generation: self.generation,
                index,
                items,
            });
        }
        packets.push(feed::Packet::LegacyComplete {
            computer: computer.into(),
            generation: self.generation,
        });
        for packet in packets {
            self.feed
                .apply(&mut self.store, &self.producer, &packet)
                .unwrap();
        }
        self.runtime.sources.native_fixture(&self.store, computer);
    }
    fn tick_until(&mut self, predicate: impl Fn(&Runtime) -> bool) {
        self.poll_until(predicate, false);
    }
    /// Collect only the original jobs. Fairness fixtures advance discovery in
    /// explicit scans; a loaded runner must not sneak in another scan during
    /// collection and replace the completed jobs before the predicate sees it.
    fn collect_until(&mut self, predicate: impl Fn(&Runtime) -> bool) {
        self.poll_until(predicate, true);
    }
    fn poll_until(&mut self, predicate: impl Fn(&Runtime) -> bool, collect_only: bool) {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if collect_only {
                self.runtime.scan_at = end;
            }
            self.runtime.poll(&mut self.store);
            if predicate(&self.runtime) {
                break;
            }
            assert!(
                Instant::now() < end,
                "delivery did not settle: {}",
                self.runtime.status()
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn scan(&mut self) {
        self.runtime.scan_at = Instant::now();
        self.runtime.poll(&mut self.store);
    }
    fn journal_status(&self) -> Option<String> {
        self.runtime
            .journal
            .as_ref()?
            .query_row("SELECT status FROM attempts LIMIT 1", [], |r| r.get(0))
            .optional()
            .unwrap()
    }
}
fn quiet() -> Runner {
    Arc::new(|_, _, _, _| panic!("unexpected native work"))
}
fn row() -> Value {
    json!({"name":"claude/example/a","cmd":"claude","created":123,"tracked":true,
        "state":"running","process_state":"running","phase":"approval","run_id":"run",
        "conversation_id":"conversation","reply_id":"reply","attention_signature":"attention"})
}
fn mark() -> reads::ReadMark {
    reads::ReadMark {
        computer: String::new(),
        name: "claude/example/a".into(),
        run_id: "run".into(),
        conversation_id: "conversation".into(),
        reply_id: Some("reply".into()),
        attention_signature: Some("attention".into()),
    }
}
fn question() -> Value {
    json!({"question_id":"q","question_hash":"a".repeat(64),"run_id":"run","conversation_id":"conversation",
        "source":"claude_tool_approval","approval":true,"can_answer":true,"approval_choices":true,
        "answer_transport":"claude_tui","questions":[{"id":"approval","options":[
            {"id":"yes","label":"Yes","scope":"once"},{"id":"always","label":"Always allow","scope":"always"}]}]})
}
fn inspected(q: Value) -> Exit {
    Exit::Code {
        code: 0,
        stderr: String::new(),
        stdout: serde_json::to_vec(&json!({"runtime_state":"live","process_state":"running",
        "run_id":"run","conversation_id":"conversation","pending_questions":[q]}))
        .unwrap(),
    }
}
fn reserve_read(f: &mut Fixture) -> reads::Delivery {
    reads::enqueue(&mut f.store.db, &id(), &mark(), now()).unwrap();
    let writer = f.runtime.writer_epoch(&f.store).unwrap().unwrap();
    reads::reserve(&mut f.store.db, &writer, &f.runtime.epoch, &id(), now())
        .unwrap()
        .unwrap()
}

#[test]
fn policy_defaults_off_and_replayed_receipts_never_reapply_old_settings() {
    let mut f = Fixture::new(quiet());
    assert!(!state::policy(&f.store.db).unwrap().enabled);
    f.active();
    let first = json!({"op":"automatic_approvals","request_id":id(),"enabled":true});
    let receipt = f.native_policy(&first);
    assert_eq!(receipt["durable"], true);
    assert_eq!(f.enabled(false)["accepted"], true);
    assert_eq!(f.native_policy(&first), receipt);
    assert!(!state::policy(&f.store.db).unwrap().enabled);
    let result=f.runtime.request(&mut f.store,&json!({"op":"delivery_result","request_id":id(),"original_request_id":first["request_id"]}));
    assert_eq!(result["result"], receipt);
    let next = id();
    drop(f.store);
    let store = Store::open(&f.location, &next).unwrap();
    assert!(!state::policy(&store.db).unwrap().enabled);
    assert!(zerus_desktop_ledger::status(&store.db).unwrap()["revoked"]
        .as_bool()
        .unwrap());
}

#[test]
fn generic_runtime_policy_requests_cannot_bypass_the_native_permit_type() {
    let mut f = Fixture::new(quiet());
    for enabled in [true, false] {
        let result = f.runtime.request(
            &mut f.store,
            &json!({"op":"automatic_approvals",
            "request_id":id(),"enabled":enabled}),
        );
        assert_eq!(result["accepted"], false);
        assert_eq!(result["code"], "frontend_authentication_required");
    }
    assert!(!state::policy(&f.store.db).unwrap().enabled);
}

#[test]
fn request_types_cross_operation_ids_and_failed_commit_refuse() {
    let mut f = Fixture::new(quiet());
    for enabled in [Value::Null, json!(1), json!("true")] {
        assert_eq!(
            f.runtime.request(
                &mut f.store,
                &json!({"op":"automatic_approvals","request_id":id(),"enabled":enabled})
            )["accepted"],
            false
        );
    }
    let request = id();
    let read = json!({"op":"read_intent","request_id":request,"mark":mark()});
    let receipt = f.runtime.request(&mut f.store, &read);
    assert_eq!(receipt["durable"], true);
    assert_eq!(f.runtime.request(&mut f.store, &read), receipt);
    assert_eq!(
        f.runtime.request(
            &mut f.store,
            &json!({"op":"automatic_approvals","request_id":request,"enabled":true})
        )["accepted"],
        false
    );
    let request = id();
    assert!(state::set_policy(&mut f.store.db, &request, false, None).is_ok());
    assert!(state::enqueue(&mut f.store.db, &request, &mark(), now()).is_err());
    f.active();
    f.store.db.execute_batch("CREATE TRIGGER fail_policy BEFORE INSERT ON desktop_delivery_requests BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert_eq!(f.enabled(true)["accepted"], false);
    assert!(!state::policy(&f.store.db).unwrap().enabled);
    f.store
        .db
        .execute_batch("DROP TRIGGER fail_policy;")
        .unwrap();
    assert_eq!(f.enabled(false)["accepted"], true);
    assert_eq!(f.enabled(true)["accepted"], true);
    assert!(f.runtime.approval_error.is_none());
}

#[test]
fn actual_root_projection_is_consumed_by_the_client_without_ready_json_fixtures() {
    use zerus_client_core::desktop::service::State;
    let mut f = Fixture::new(quiet());
    let mut client = State::new(f.location.configuration.clone()).unwrap();
    let mut output = Vec::new();
    // A passive service: no background runtime decorates its lifecycle, so
    // all five components stay not_started (the Windows passive GUI state).
    let passive = f.passive_projection();
    client.status(&passive).unwrap();
    assert!(client.snapshot().connected);
    assert!(!client.snapshot().background_ready());
    for component in ["fleet", "presence", "reads", "attention", "approvals"] {
        assert_eq!(
            passive["lifecycle"]["background"][component]["code"], "not_started",
            "{component}"
        );
    }
    output.push(json!({"case":"passive","status":passive}));
    let mut check = |case: &str, f: &Fixture, reads: bool, approvals: bool| {
        let value = f.projection();
        client.status(&value).unwrap();
        assert!(client.snapshot().connected);
        assert_eq!(client.snapshot().component_ready("reads"), reads, "{case}");
        assert_eq!(
            client.snapshot().component_ready("approvals"),
            approvals,
            "{case}"
        );
        output.push(json!({"case":case,"status":value}));
    };
    check("awaiting_handoff", &f, false, false);
    f.active();
    check("disabled_but_ready", &f, true, true);
    assert_eq!(
        f.projection()["lifecycle"]["background"]["approvals"]["code"],
        "disabled"
    );
    assert_eq!(
        f.projection()["lifecycle"]["background"]["approvals"]["enabled"],
        false
    );
    assert_eq!(f.enabled(true)["durable"], true);
    check("enabled", &f, true, true);
    f.runtime.approval_error = Some("fixture_approval_unavailable");
    check("approval_degraded", &f, true, false);
    assert_eq!(
        f.projection()["automatic_approvals"],
        true,
        "durable opt-in is not worker readiness"
    );
    assert_eq!(f.enabled(false)["durable"], true);
    assert!(!state::policy(&f.store.db).unwrap().enabled);
    check("disabled_while_degraded", &f, true, false);
    f.runtime.approval_error = None;
    f.runtime.read_error = Some("fixture_read_unavailable");
    check("read_degraded", &f, false, true);
    f.runtime.request_stop();
    check("drained", &f, false, false);
    assert_eq!(
        f.projection()["lifecycle"]["background"]["approvals"]["code"],
        "drained"
    );
    println!(
        "ROOT_DELIVERY_STATUS {}",
        serde_json::to_string(&output).unwrap()
    );
}

#[test]
fn enable_requires_current_writer_even_when_cached_projection_was_ready() {
    let mut f = Fixture::new(quiet());
    assert_eq!(f.enabled(true)["accepted"], false);
    f.active();
    assert_eq!(
        f.projection()["lifecycle"]["background"]["approvals"]["ready"],
        true
    );
    zerus_desktop_ledger::revoke(&f.store.db, "fixture").unwrap();
    // No intervening poll: the display cache must grant no authority.
    assert_eq!(f.enabled(true)["accepted"], false);
    assert!(!state::policy(&f.store.db).unwrap().enabled);
    assert_eq!(f.enabled(false)["durable"], true);
    f.runtime.request_stop();
    assert_eq!(f.enabled(false)["durable"], true);
}

#[test]
fn degraded_runtime_returns_original_receipt_without_reapplying_old_enable() {
    let mut f = Fixture::new(quiet());
    f.active();
    let original = json!({"op":"automatic_approvals","request_id":id(),"enabled":true});
    let receipt = f.native_policy(&original);
    assert_eq!(receipt["durable"], true);
    assert_eq!(f.enabled(false)["durable"], true);
    f.runtime.request_stop();
    assert_eq!(f.native_policy(&original), receipt);
    assert!(!state::policy(&f.store.db).unwrap().enabled);
    assert_eq!(f.enabled(true)["accepted"], false);
}

#[test]
fn passive_disable_requires_typed_authentication_and_an_actual_store_commit() {
    let mut f = Fixture::new(quiet());
    state::set_policy(&mut f.store.db, &id(), true, None).unwrap();
    let passive = f.passive_projection();
    assert_eq!(passive["automatic_approvals"], true);
    assert_eq!(
        passive["lifecycle"]["background"]["approvals"]["enabled"],
        true
    );
    assert_eq!(
        passive["lifecycle"]["background"]["approvals"]["ready"],
        false
    );
    let mut client =
        zerus_client_core::desktop::service::State::new(f.location.configuration.clone()).unwrap();
    client.status(&passive).unwrap();
    assert_eq!(client.snapshot().automatic_approvals, Some(true));
    assert!(!client.snapshot().component_ready("approvals"));
    let request = |enabled| json!({"op":"automatic_approvals","request_id":id(),"enabled":enabled});
    let apply = |store: &mut Store, request: &Value| {
        passive_frontend_request(
            store,
            crate::desktop::ui_authority::Validated::fixture(request),
        )
    };
    assert_eq!(apply(&mut f.store, &request(true))["accepted"], false);
    assert!(state::policy(&f.store.db).unwrap().enabled);
    f.store.db.execute_batch("CREATE TRIGGER deny_disable BEFORE UPDATE ON desktop_delivery_policy BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let original = request(false);
    assert_eq!(apply(&mut f.store, &original)["durable"], false);
    assert!(state::policy(&f.store.db).unwrap().enabled);
    f.store
        .db
        .execute_batch("DROP TRIGGER deny_disable;")
        .unwrap();
    let receipt = apply(&mut f.store, &original);
    assert_eq!(receipt["durable"], true);
    assert_eq!(apply(&mut f.store, &original), receipt);
    assert!(!state::policy(&f.store.db).unwrap().enabled);
    assert_eq!(
        f.runtime.request(&mut f.store, &request(false))["code"],
        "frontend_authentication_required"
    );
}

fn category_request(enabled: bool, ids: &[&str]) -> Value {
    json!({"op":"automatic_approvals","request_id":id(),"enabled":enabled,"categories":ids})
}
const DEFAULT_IDS: [&str; 7] = ["read", "edit", "shell", "web", "mcp", "agents", "other"];

#[test]
fn categories_default_to_all_but_paid_and_change_only_through_exact_receipts() {
    let mut f = Fixture::new(quiet());
    let policy = state::policy(&f.store.db).unwrap();
    assert_eq!(policy.categories.ids(), DEFAULT_IDS);
    assert!(!policy.enabled);
    // An older leaf's policy row gains the same default; the leaf is idempotent.
    f.store.db.execute_batch("ALTER TABLE desktop_delivery_policy DROP COLUMN categories;").unwrap();
    assert!(state::policy(&f.store.db).is_err());
    migrate_categories(&f.store.db).unwrap();
    migrate_categories(&f.store.db).unwrap();
    assert_eq!(state::policy(&f.store.db).unwrap().categories.ids(), DEFAULT_IDS);
    f.active();
    assert_eq!(f.enabled(true)["accepted"], true);
    // Widening while on needs ready workers, which the active fixture has.
    let mut all = DEFAULT_IDS.to_vec();
    all.push("paid");
    let request = category_request(true, &all);
    let receipt = f.native_policy(&request);
    assert_eq!((receipt["accepted"].clone(), receipt["categories"].clone()), (json!(true), json!(all)));
    assert_eq!(f.native_policy(&request), receipt);
    let policy = state::policy(&f.store.db).unwrap();
    assert!(policy.enabled && policy.categories.contains(policy::Category::Paid));
    let mut projected = json!({});
    f.runtime.decorate(&mut projected);
    assert_eq!(projected["automatic_approval_categories"], json!(all));
    // The same request ID with another set is another intent.
    let mut changed = request.clone();
    changed["categories"] = json!(["read"]);
    assert_eq!(f.native_policy(&changed)["accepted"], false);
    // A category change never turns the switch on or off.
    assert_eq!(f.native_policy(&category_request(false, &["read"]))["code"], "approval_policy_changed");
    assert!(state::policy(&f.store.db).unwrap().enabled);
    // Unknown or repeated IDs are refused before any write.
    for ids in [vec!["read", "read"], vec!["browser"]] {
        assert_eq!(f.native_policy(&category_request(true, &ids))["accepted"], false);
    }
    assert_eq!(state::policy(&f.store.db).unwrap().categories.ids().len(), 8);
    // Turning the switch off keeps the categories.
    assert_eq!(f.enabled(false)["accepted"], true);
    assert_eq!(state::policy(&f.store.db).unwrap().categories.ids().len(), 8);
}

#[test]
fn passive_category_changes_may_only_reduce_what_is_approved() {
    let mut f = Fixture::new(quiet());
    state::set_policy(&mut f.store.db, &id(), true, None).unwrap();
    let apply = |store: &mut Store, request: &Value| {
        passive_frontend_request(store, crate::desktop::ui_authority::Validated::fixture(request))
    };
    let mut all = DEFAULT_IDS.to_vec();
    all.push("paid");
    // Adding Paid usage while approvals are on needs the background workers.
    assert_eq!(apply(&mut f.store, &category_request(true, &all))["code"], "desktop_background_unavailable");
    assert!(!state::policy(&f.store.db).unwrap().categories.contains(policy::Category::Paid));
    // Removing Shell commands reaches the store without them.
    let narrower: Vec<&str> = DEFAULT_IDS.iter().copied().filter(|id| *id != "shell").collect();
    let receipt = apply(&mut f.store, &category_request(true, &narrower));
    assert_eq!(receipt["durable"], true);
    assert_eq!(state::policy(&f.store.db).unwrap().categories.ids(), narrower);
    let passive = f.passive_projection();
    assert_eq!(passive["automatic_approval_categories"], json!(narrower));
    let mut client =
        zerus_client_core::desktop::service::State::new(f.location.configuration.clone()).unwrap();
    client.status(&passive).unwrap();
    assert_eq!(client.snapshot().automatic_approval_categories, Some(narrower.iter().map(|s| s.to_string()).collect()));
    // A malformed stored set fails closed, yet never blocks turning off.
    f.store.db.execute("UPDATE desktop_delivery_policy SET categories='[\"browser\"]'", []).unwrap();
    assert!(state::policy(&f.store.db).is_err());
    assert_eq!(apply(&mut f.store, &category_request(true, &["read"]))["accepted"], false);
    assert_eq!(apply(&mut f.store, &json!({"op":"automatic_approvals","request_id":id(),"enabled":false}))["durable"], true);
    f.store.db.execute("UPDATE desktop_delivery_policy SET categories='[\"read\"]', enabled=1", []).unwrap();
    // While approvals are off, any category change approves nothing now.
    assert_eq!(apply(&mut f.store, &json!({"op":"automatic_approvals","request_id":id(),"enabled":false}))["durable"], true);
    assert_eq!(apply(&mut f.store, &category_request(false, &all))["durable"], true);
    assert_eq!(apply(&mut f.store, &json!({"op":"automatic_approvals","request_id":id(),"enabled":true}))["accepted"], false);
}

#[test]
fn display_feed_cannot_admit_delivery_or_retire_a_read() {
    let mut f = Fixture::new(quiet());
    f.active();
    reads::enqueue(&mut f.store.db, &id(), &mark(), now()).unwrap();
    let mut seen = row();
    seen["read"] = json!({"conversation_id":"conversation","reply_id":"reply","attention_signature":"attention"});
    f.source(seen, false);
    assert!(f.runtime.sources.eligible(&f.store).unwrap().is_empty());
    assert!(f.runtime.sources.page(&mut f.store).unwrap().is_empty());
    assert_eq!(reads::status(&f.store.db).unwrap()["pending"], 1);
    f.runtime.sources.native_fixture(&f.store, "");
    f.runtime.sources.observe_reads(&mut f.store).unwrap();
    assert_eq!(reads::status(&f.store.db).unwrap()["pending"], 0);
}

#[test]
fn failed_read_worker_retains_the_original_ticket_until_finish_commits() {
    let mut f = Fixture::new(quiet());
    f.active();
    let delivery = reserve_read(&mut f);
    let lease = f.runtime.lease(Kind::Read).unwrap();
    f.runtime
        .jobs
        .push(Job::not_started(Phase::Read(delivery.clone()), lease));
    f.store.db.execute_batch("CREATE TRIGGER fail_finish BEFORE UPDATE ON desktop_read_attempts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    f.runtime.poll(&mut f.store);
    assert_eq!(f.runtime.jobs.len(), 1);
    assert_eq!(reads::status(&f.store.db).unwrap()["in_flight"], 1);
    f.store
        .db
        .execute_batch("DROP TRIGGER fail_finish;")
        .unwrap();
    f.runtime.poll(&mut f.store);
    assert!(f.runtime.jobs.is_empty());
    assert_eq!(reads::status(&f.store.db).unwrap()["in_flight"], 0);
    assert!(f.runtime.read_error.is_none());
    let result: String = f
        .store
        .db
        .query_row(
            "SELECT result FROM desktop_read_attempts WHERE id=?1",
            [delivery.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<reads::Outcome>(&result).unwrap(),
        reads::Outcome::NotSent
    );
}

#[test]
fn cancelled_reserved_answer_retries_only_its_journal_finish() {
    cancelled_answer_finish_with_due_scan(false);
}

#[test]
fn cancelled_reserved_answer_finish_survives_stop_without_new_inspection() {
    cancelled_answer_finish_with_due_scan(true);
}

fn cancelled_answer_finish_with_due_scan(stopping: bool) {
    let inspections = Arc::new(AtomicUsize::new(0));
    let answers = Arc::new(AtomicUsize::new(0));
    let run: Runner = {
        let inspections = inspections.clone();
        let answers = answers.clone();
        Arc::new(move |inv, _, _, _| {
            if inv.args.iter().any(|arg| arg == "inspect") {
                inspections.fetch_add(1, Ordering::SeqCst);
                inspected(question())
            } else {
                answers.fetch_add(1, Ordering::SeqCst);
                panic!("a cancelled Answer must never be replayed");
            }
        })
    };
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    f.source(row(), true);
    let candidate = f.runtime.sources.page(&mut f.store).unwrap().remove(0);
    let mut journal = approval_log::open(&f.runtime.approval_root).unwrap();
    let r = approval_log::begin(
        &mut journal,
        &json!({"host":"local","session":mark().name,"provider":"claude","question":question()}),
    )
    .unwrap();
    let reserved = policy::reserved(&r, "claude", &question())
        .unwrap()
        .unwrap();
    let original_id = reserved.id;
    let original_request = reserved.request.clone();
    journal.execute_batch("CREATE TRIGGER fail_finish BEFORE UPDATE ON attempts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    f.runtime.journal = Some(journal);
    let lease = f.runtime.lease(Kind::Approval).unwrap();
    f.runtime.jobs.push(Job::not_started(
        Phase::Answer {
            candidate,
            audit: Audit {
                id: reserved.id,
                request: reserved.request,
            },
        },
        lease,
    ));
    if stopping {
        f.runtime.request_stop();
    }
    // Poll also discovers new work. Make that boundary deterministic instead
    // of depending on setup finishing within the one-second scan interval.
    f.runtime.scan_at = Instant::now();
    f.runtime.poll(&mut f.store);
    assert_eq!(f.journal_status().as_deref(), Some("prepared"));
    assert_eq!(f.runtime.jobs.len(), 1);
    assert!(
        matches!(&f.runtime.jobs[0].phase, Phase::Answer { audit, .. }
        if audit.id == original_id && audit.request == original_request)
    );
    assert_eq!(
        f.runtime.approval_error,
        Some("approval_receipt_uncommitted")
    );
    assert_eq!(inspections.load(Ordering::SeqCst), 0);
    assert_eq!(answers.load(Ordering::SeqCst), 0);
    f.runtime
        .journal
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_finish;")
        .unwrap();
    f.runtime.scan_at = Instant::now();
    f.runtime.poll(&mut f.store);
    assert_eq!(f.journal_status().as_deref(), Some("cancelled"));
    assert!(f.runtime.approval_error.is_none());
    if stopping {
        assert!(f.runtime.drained());
    } else {
        // The original Answer is finished. Its removal and recovered journal
        // permit a FRESH Inspect in this same poll, never an Answer retry.
        assert_eq!(f.runtime.jobs.len(), 1);
        assert!(matches!(&f.runtime.jobs[0].phase, Phase::Inspect(_)));
        // Finish this worker with policy/writer admission still active. The
        // same question must find its cancelled reservation, not send Answer.
        // Keep later discovery outside tick_until's bounded five-second wait.
        f.runtime.scan_at = Instant::now() + Duration::from_secs(60);
        f.tick_until(|runtime| runtime.jobs.is_empty());
        f.runtime.request_stop();
        assert!(f.runtime.drained());
    }
    let journal = f.runtime.journal.as_ref().unwrap();
    let entry: (i64, String, String, String) = journal
        .query_row(
            "SELECT id,request_id,status,detail FROM attempts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        entry,
        (
            original_id,
            original_request,
            "cancelled".into(),
            policy::CANCELLED.into()
        )
    );
    assert_eq!(
        journal
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(inspections.load(Ordering::SeqCst), usize::from(!stopping));
    assert_eq!(answers.load(Ordering::SeqCst), 0);
}

#[test]
fn approval_reservation_precedes_answer_and_unknown_never_replays_after_restart() {
    let answers = Arc::new(AtomicUsize::new(0));
    let count = answers.clone();
    let journal_path = Arc::new(Mutex::new(PathBuf::new()));
    let path = journal_path.clone();
    let run: Runner = Arc::new(move |inv, _, _, _| {
        if inv.args.iter().any(|a| a == "inspect") {
            return inspected(question());
        }
        let audit = Connection::open(path.lock().unwrap().as_path()).unwrap();
        let (status, request): (String, String) = audit
            .query_row("SELECT status,request_id FROM attempts LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(status, "prepared");
        assert_eq!(inv.request_id.as_deref(), Some(request.as_str()));
        let input: Value = serde_json::from_slice(inv.stdin.as_deref().unwrap()).unwrap();
        assert_eq!(input["expected_run_id"], "run");
        assert_eq!(input["expected_conversation_id"], "conversation");
        assert_eq!(input["expected_question_hash"], "a".repeat(64));
        count.fetch_add(1, Ordering::SeqCst);
        Exit::TimedOut
    });
    let mut f = Fixture::new(run.clone());
    *journal_path.lock().unwrap() = f.runtime.approval_root.join("journal.sqlite3");
    f.active();
    f.enabled(true);
    f.source(row(), true);
    f.scan();
    f.tick_until(|r| r.jobs.is_empty());
    assert_eq!(answers.load(Ordering::SeqCst), 1);
    assert_eq!(f.journal_status().as_deref(), Some("unknown"));
    let mut replacement = Runtime::new(&f.runtime.epoch, f.runtime.approval_root.clone(), run);
    replacement.sources.native_fixture(&f.store, "");
    f.runtime = replacement;
    f.scan();
    f.tick_until(|r| r.jobs.is_empty());
    assert_eq!(answers.load(Ordering::SeqCst), 1);
    assert_eq!(f.journal_status().as_deref(), Some("unknown"));
}

#[test]
fn paid_requests_stay_manual_and_others_record_their_categories() {
    // The service admits the default categories: all except paid usage.
    // Case 3 turns Paid usage on, which admits the paid search.
    for (case, header) in ["WebSearch", "Bash", "Bash", "WebSearch"].into_iter().enumerate() {
        let run: Runner = Arc::new(move |inv, _, _, _| {
            if inv.args.iter().any(|a| a == "inspect") {
                let mut q = question();
                q["questions"][0]["header"] = json!(header);
                let Exit::Code { stdout, .. } = inspected(q) else { unreachable!() };
                let mut details: Value = serde_json::from_slice(&stdout).unwrap();
                if case == 2 {
                    details["cache_hint"] = json!({"status":"cold"});
                }
                return Exit::Code { code: 0, stderr: String::new(), stdout: serde_json::to_vec(&details).unwrap() };
            }
            assert!(case == 1 || case == 3, "a paid request was answered");
            Exit::TimedOut
        });
        let mut f = Fixture::new(run);
        f.active();
        f.enabled(true);
        if case == 3 {
            let mut all = DEFAULT_IDS.to_vec();
            all.push("paid");
            assert_eq!(f.native_policy(&category_request(true, &all))["accepted"], true);
        }
        f.source(row(), true);
        f.scan();
        f.tick_until(|r| r.jobs.is_empty());
        if case == 1 || case == 3 {
            let saved: String = f.runtime.journal.as_ref().unwrap()
                .query_row("SELECT question FROM attempts", [], |r| r.get(0)).unwrap();
            let saved: Value = serde_json::from_str(&saved).unwrap();
            assert_eq!(saved["categories"], if case == 1 { json!(["shell"]) } else { json!(["paid"]) });
        } else {
            assert!(f.runtime.journal.is_none(), "{header} case {case}");
        }
    }
}

#[test]
fn a_cache_that_expires_after_inspection_is_not_reserved_without_paid_usage() {
    // Review of PR #1694: the actor rechecks at the current time, not with
    // the categories the inspection computed. The same flow reserves when
    // the cache stays warm (control) and not once it has expired.
    for (ttl, wait) in [(60.0, 0), (2.0, 2100)] {
        let run: Runner = Arc::new(move |inv, _, _, _| {
            if !inv.args.iter().any(|a| a == "inspect") {
                assert_eq!(ttl, 60.0, "an expired request was answered");
                return Exit::TimedOut;
            }
            let mut q = question();
            q["questions"][0]["header"] = json!("Bash");
            let Exit::Code { stdout, .. } = inspected(q) else { unreachable!() };
            let mut details: Value = serde_json::from_slice(&stdout).unwrap();
            let expiry = policy::now().floor() + ttl;
            details["session_usage"] = json!({"prompt_cache":{"status":"warm","expires_at":expiry}});
            Exit::Code { code: 0, stderr: String::new(), stdout: serde_json::to_vec(&details).unwrap() }
        });
        let mut f = Fixture::new(run);
        f.active();
        f.enabled(true);
        f.source(row(), true);
        f.scan();
        // Let the inspection select the warm request, then let it expire
        // before the actor collects the result.
        let end = Instant::now() + Duration::from_secs(5);
        while !f.runtime.jobs.iter().all(|job| job.thread.as_ref().is_some_and(|t| t.is_finished())) {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(2));
        }
        thread::sleep(Duration::from_millis(wait));
        f.tick_until(|r| r.jobs.is_empty());
        assert_eq!(f.runtime.journal.is_some(), wait == 0, "ttl {ttl}");
    }
}

#[test]
fn cached_question_identity_and_broader_grants_never_reserve_or_answer() {
    for case in 0..5 {
        let run: Runner = Arc::new(move |inv, _, _, _| {
            assert!(inv.args.iter().any(|a| a == "inspect"));
            let mut q = question();
            match case {
                0 => q["run_id"] = json!("stale"),
                1 => q["conversation_id"] = json!("stale"),
                2 => q["trust_request"] = json!(true),
                3 => q["questions"][0]["options"][0]["scope"] = json!("always"),
                _ => q["approval"] = json!(false),
            }
            inspected(q)
        });
        let mut f = Fixture::new(run);
        f.active();
        f.enabled(true);
        f.source(row(), true);
        f.scan();
        f.tick_until(|r| r.jobs.is_empty());
        assert!(f.runtime.journal.is_none());
    }
}

#[test]
fn disabling_during_inspection_cancels_before_reservation() {
    let release = Arc::new(AtomicBool::new(false));
    let gate = release.clone();
    let run: Runner = Arc::new(move |_, _, _, _| {
        let end = Instant::now() + Duration::from_secs(3);
        while !gate.load(Ordering::Acquire) {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(1));
        }
        inspected(question())
    });
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    f.source(row(), true);
    f.scan();
    assert_eq!(f.runtime.jobs.len(), 1);
    assert_eq!(f.enabled(false)["accepted"], true);
    release.store(true, Ordering::Release);
    f.tick_until(|r| r.jobs.is_empty());
    assert!(f.runtime.journal.is_none());
}

#[test]
fn retained_native_resources_hold_slots_and_prevent_a_false_drained_status() {
    let mut f = Fixture::new(quiet());
    let mut resources = Vec::new();
    for _ in 0..READS {
        resources.push(f.runtime.lease(Kind::Read).unwrap());
    }
    assert!(f.runtime.lease(Kind::Read).is_none());
    for _ in 0..APPROVALS {
        resources.push(f.runtime.lease(Kind::Approval).unwrap());
    }
    assert_eq!(f.runtime.status()["retained_slots"], SLOTS);
    assert!(f.runtime.lease(Kind::Approval).is_none());
    f.runtime.request_stop();
    assert!(!f.runtime.drained());
    drop(resources);
    assert!(f.runtime.drained());
}

#[test]
fn read_receipt_does_not_retire_intent_until_native_catalog_confirms_it() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let run: Runner = Arc::new(move |inv, _, _, _| {
        assert!(inv.args.iter().any(|a| a == "read"));
        let input: Value = serde_json::from_slice(inv.stdin.as_deref().unwrap()).unwrap();
        assert_eq!(input["run_id"], "run");
        assert_eq!(input["conversation_id"], "conversation");
        count.fetch_add(1, Ordering::SeqCst);
        Exit::Code {
            code: 0,
            stderr: String::new(),
            stdout: serde_json::to_vec(
                &json!({"ok":true,"applied":{"reply_id":true,"attention_signature":true}}),
            )
            .unwrap(),
        }
    });
    let mut f = Fixture::new(run);
    f.active();
    f.source(row(), true);
    state::enqueue(&mut f.store.db, &id(), &mark(), now()).unwrap();
    f.runtime.poll(&mut f.store);
    f.tick_until(|r| r.jobs.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(reads::status(&f.store.db).unwrap()["pending"], 1);
    let mut seen = row();
    seen["read"] = json!({"conversation_id":"conversation","reply_id":"reply","attention_signature":"attention"});
    f.source(seen, true);
    f.runtime.sources.observe_reads(&mut f.store).unwrap();
    assert_eq!(reads::status(&f.store.db).unwrap()["pending"], 0);
}

#[test]
fn imported_frozen_marks_use_the_original_identity_guarded_delivery_path_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let run: Runner = Arc::new(move |inv, _, _, _| {
        assert!(inv.args.iter().any(|a| a == "read"));
        assert!(inv.args.iter().any(|a| a == "claude/example/a"));
        let input: Value = serde_json::from_slice(inv.stdin.as_deref().unwrap()).unwrap();
        assert_eq!(input["run_id"], "run");
        assert_eq!(input["conversation_id"], "conversation");
        assert_eq!(input["reply_id"], "reply");
        assert_eq!(input["attention_signature"], "attention");
        count.fetch_add(1, Ordering::SeqCst);
        Exit::Code {
            code: 0,
            stderr: String::new(),
            stdout: br#"{"ok":true,"applied":{"reply_id":true,"attention_signature":true}}"#
                .to_vec(),
        }
    });
    let mut f = Fixture::new(run);
    let capture = zerus_client_core::desktop::pending_reads::Capture::from_bytes(
        &serde_json::to_vec(&vec![mark()]).unwrap(),
    )
    .unwrap();
    let page = capture.page(0).unwrap();
    let request = || json!({"op":"import_pending_reads","request_id":id(),"page":page});
    assert_eq!(
        crate::desktop::pending_reads::frontend(
            &mut f.store,
            crate::desktop::ui_authority::Validated::fixture(&request())
        )["durable"],
        true
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!state::policy(&f.store.db).unwrap().enabled);
    f.source(row(), true);
    f.active();
    f.tick_until(|r| r.jobs.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(reads::status(&f.store.db).unwrap()["pending"], 1);
    let mut seen = row();
    seen["read"] = json!({"conversation_id":"conversation","reply_id":"reply","attention_signature":"attention"});
    f.source(seen, true);
    f.runtime.sources.observe_reads(&mut f.store).unwrap();
    assert_eq!(reads::status(&f.store.db).unwrap()["pending"], 0);
    assert_eq!(
        crate::desktop::pending_reads::frontend(
            &mut f.store,
            crate::desktop::ui_authority::Validated::fixture(&request())
        )["durable"],
        true
    );
    f.scan();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn pending_observation_is_fair_bounded_and_indexed() {
    let mut f = Fixture::new(quiet());
    let m = mark();
    for n in 0..33 {
        let mut old = m.clone();
        old.name = format!("unlisted-{n}");
        reads::enqueue(&mut f.store.db, &id(), &old, now()).unwrap();
    }
    let mut old = m.clone();
    old.run_id = "superseded".into();
    reads::enqueue(&mut f.store.db, &id(), &old, now()).unwrap();
    f.source(row(), true);
    for _ in 0..2 {
        f.runtime.sources.observe_reads(&mut f.store).unwrap();
    }
    assert_eq!(reads::status(&f.store.db).unwrap()["pending"], 34);
    f.runtime.sources.observe_reads(&mut f.store).unwrap();
    assert_eq!(reads::status(&f.store.db).unwrap()["pending"], 33);
    let plan:String=f.store.db.query_row("EXPLAIN QUERY PLAN SELECT json FROM desktop_catalog_rows WHERE generation=1 AND CASE WHEN json_valid(json) THEN json_extract(json,'$.name') END='a' ORDER BY id LIMIT 2",[],|r|r.get(3)).unwrap();
    assert!(plan.contains("desktop_delivery_catalog_name"), "{plan}");
}

#[test]
fn a_valid_inspection_prefix_cannot_release_retained_native_resources() {
    let held = Arc::new(Mutex::new(None));
    let pin = held.clone();
    let run: Runner = Arc::new(move |_, _, _, lease| {
        *pin.lock().unwrap() = Some(lease);
        inspected(question())
    });
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    f.source(row(), true);
    f.scan();
    f.tick_until(|r| r.jobs.is_empty());
    assert!(f.runtime.journal.is_none());
    assert_eq!(f.runtime.status()["retained_slots"], 1);
    f.runtime.request_stop();
    assert!(!f.runtime.drained());
    held.lock().unwrap().take();
    assert!(f.runtime.drained());
}

#[test]
fn replacing_the_writer_epoch_cancels_an_already_started_inspection() {
    let release = Arc::new(AtomicBool::new(false));
    let gate = release.clone();
    let run: Runner = Arc::new(move |_, _, _, _| {
        let end = Instant::now() + Duration::from_secs(3);
        while !gate.load(Ordering::Acquire) {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(1));
        }
        inspected(question())
    });
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    f.source(row(), true);
    f.scan();
    assert_eq!(f.runtime.jobs.len(), 1);
    zerus_desktop_ledger::authorize(&mut f.store.db, &id(), &f.runtime.epoch).unwrap();
    f.runtime.poll(&mut f.store);
    release.store(true, Ordering::Release);
    f.tick_until(|r| r.jobs.is_empty());
    assert!(f.runtime.journal.is_none());
}

#[test]
fn persistence_retry_does_not_upgrade_an_original_incomplete_read_receipt() {
    let held = Arc::new(Mutex::new(None));
    let pin = held.clone();
    let run: Runner = Arc::new(move |_, _, _, lease| {
        *pin.lock().unwrap() = Some(lease);
        Exit::Code {
            code: 0,
            stderr: String::new(),
            stdout: serde_json::to_vec(&json!({"ok":true,
            "applied":{"reply_id":true,"attention_signature":true}}))
            .unwrap(),
        }
    });
    let mut f = Fixture::new(run);
    f.active();
    f.source(row(), true);
    reads::enqueue(&mut f.store.db, &id(), &mark(), now()).unwrap();
    f.store.db.execute_batch("CREATE TRIGGER fail_finish BEFORE UPDATE ON desktop_read_attempts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    f.runtime.poll(&mut f.store);
    f.tick_until(|r| r.jobs.first().is_some_and(|job| job.result.is_some()));
    held.lock().unwrap().take();
    f.store
        .db
        .execute_batch("DROP TRIGGER fail_finish;")
        .unwrap();
    f.runtime.poll(&mut f.store);
    assert!(f.runtime.jobs.is_empty());
    let result: String = f
        .store
        .db
        .query_row(
            "SELECT result FROM desktop_read_attempts LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<reads::Outcome>(&result).unwrap(),
        reads::Outcome::Failed
    );
}

/// Idle sessions, then the fixture's approval row last (`claude/example/a`).
fn idle_then_approval(idle: usize) -> Vec<CatalogItem> {
    let mut rows: Vec<CatalogItem> = (0..idle)
        .map(|n| {
            let mut row = row();
            row["name"] = json!(format!("claude/example/idle-{n}"));
            row["phase"] = json!("working");
            row["run_id"] = json!(format!("run-{n}"));
            row["conversation_id"] = json!(format!("conversation-{n}"));
            CatalogItem {
                key: format!("run:run-{n}"),
                row,
            }
        })
        .collect();
    rows.push(CatalogItem {
        key: "run:run".into(),
        row: row(),
    });
    rows
}

/// The sessions whose native inspection the runner was asked for.
fn inspections() -> (Runner, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let run: Runner = Arc::new(move |inv, _, _, _| {
        assert!(inv.args.iter().any(|a| a == "inspect"));
        record.lock().unwrap().extend(inv.args.iter().cloned());
        Exit::TimedOut
    });
    (run, seen)
}

#[test]
fn every_row_the_policy_accepts_is_in_the_approval_index() {
    // zerus-5wmh.66.3.33: the index may hold more rows than the policy
    // accepts, never fewer, so indexing changes no eligibility.
    let (mut accepted, mut only_indexed) = (0, 0);
    for provider in ["claude", "opencode", "dsh", "codex", ""] {
        for phase in [
            json!("approval"),
            json!("input"),
            json!("question"),
            json!("working"),
            Value::Null,
        ] {
            for child in [
                None,
                Some(json!({"phase":"approval"})),
                Some(json!({"state":"approval"})),
                Some(json!({"display_state":"approval"})),
                Some(json!({"status":"Needs approval"})),
                Some(json!({"phase":"working","status":"Running"})),
            ] {
                for (tracked, state) in [(true, "running"), (false, "running"), (true, "stopped")] {
                    let mut row = row();
                    row["cmd"] = json!(provider);
                    row["phase"] = phase.clone();
                    row["tracked"] = json!(tracked);
                    row["state"] = json!(state);
                    if let Some(child) = &child {
                        row["subagents"] = json!({"agent-1": child});
                    }
                    let candidate =
                        policy::candidate("", &zerus_client_core::view::row(&row)).is_some();
                    let indexed = zerus_desktop_catalog::approvals::approval_phase(&row);
                    assert!(!candidate || indexed, "{row}");
                    accepted += usize::from(candidate);
                    only_indexed += usize::from(indexed && !candidate);
                }
            }
        }
    }
    assert!(
        accepted > 0 && only_indexed > 0,
        "{accepted} {only_indexed}"
    );
}

#[test]
fn an_approval_at_the_end_of_a_large_catalog_is_found_on_the_first_scan() {
    // The one-page walk would reach the last of 20,001 rows after 157 scans.
    let (run, seen) = inspections();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    f.source_rows(idle_then_approval(20_000));
    f.scan();
    f.tick_until(|r| r.jobs.is_empty());
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|a| a == "claude/example/a"), "{seen:?}");
    assert!(!seen.iter().any(|a| a.starts_with("claude/example/idle-")));
}

#[test]
fn a_source_older_than_fresh_is_not_scanned_until_it_is_observed_again() {
    // zerus-e04p, #1357 review: a source last observed more than FRESH ago,
    // without a heartbeat, takes no part in discovery.
    let (run, seen) = inspections();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    f.source_rows(idle_then_approval(1));
    f.runtime.sources.age_fixture(Duration::from_secs(16));
    f.scan();
    f.tick_until(|r| r.jobs.is_empty());
    assert!(seen.lock().unwrap().is_empty());
    f.runtime.sources.heartbeat_fixture(&f.store);
    f.scan();
    f.tick_until(|r| r.jobs.is_empty());
    assert!(seen.lock().unwrap().iter().any(|a| a == "claude/example/a"));
}

#[test]
fn rows_stored_before_the_index_are_found_once_the_scan_backfills_them() {
    let (run, seen) = inspections();
    let mut f = Fixture::new(run.clone());
    f.active();
    f.enabled(true);
    f.source_rows(idle_then_approval(300));
    // An upgraded store: every row predates schema 22, then a restart.
    f.store
        .db
        .execute_batch("UPDATE desktop_catalog_rows SET approval=NULL;")
        .unwrap();
    let mut restarted = Runtime::new(&f.runtime.epoch, f.runtime.approval_root.clone(), run);
    restarted.sources.native_fixture(&f.store, "");
    f.runtime = restarted;
    f.scan();
    f.tick_until(|r| r.jobs.is_empty());
    assert!(seen.lock().unwrap().iter().any(|a| a == "claude/example/a"));
    let pending: i64 = f
        .store
        .db
        .query_row(
            "SELECT count(*) FROM desktop_catalog_rows WHERE approval IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0);
    // The next scan finds nothing left and stops backfilling.
    f.scan();
    assert!(f.runtime.backfilled);
}

/// Rows the approval policy reads: `name` in `phase`, keyed by its run.
fn named(name: &str, phase: &str, tracked: bool) -> CatalogItem {
    let mut row = row();
    row["name"] = json!(format!("claude/example/{name}"));
    row["phase"] = json!(phase);
    row["tracked"] = json!(tracked);
    row["run_id"] = json!(format!("run-{name}"));
    row["conversation_id"] = json!(format!("conversation-{name}"));
    CatalogItem {
        key: format!("run:run-{name}"),
        row,
    }
}

/// Idle rows first: the one-page catalog walk needs 32 scans to pass them,
/// so within these tests only the approval-phase index reaches what follows.
fn idle(count: usize) -> Vec<CatalogItem> {
    (0..count)
        .map(|n| named(&format!("idle-{n}"), "working", true))
        .collect()
}

impl Fixture {
    /// A live source's heartbeat, one scan with its inspections settled, then
    /// a second passes: the 5 s inspection cooldown counts it. The heartbeat
    /// keeps a test's sources current however slow the runner; without it,
    /// more than `FRESH` of real time made every later scan skip them.
    fn second(&mut self) {
        self.runtime.sources.heartbeat_fixture(&self.store);
        self.scan();
        self.tick_until(|r| r.jobs.is_empty());
        for (_, at) in self.runtime.recent.iter_mut() {
            *at = at.checked_sub(Duration::from_secs(1)).unwrap_or(*at);
        }
    }
}

#[test]
fn questions_that_need_manual_trust_cannot_keep_a_later_indexed_row_waiting() {
    // Review of #1191: 96 flagged sessions whose native questions need manual
    // trust, and only the fourth one's could be approved. With two approval
    // slots, rows past the second of each indexed page were dropped while the
    // pass moved on, and the same first rows came back as their cooldown ran
    // out: the fourth row was never inspected. A pass now moves only past
    // rows it inspected or decided.
    let (run, seen) = inspections();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    let mut rows = idle(4_000);
    rows.extend((0..96).map(|n| named(&format!("ask-{n}"), "approval", true)));
    f.source_rows(rows);
    for _ in 0..18 {
        f.second();
    }
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|a| a == "claude/example/ask-3"), "{seen:?}");
    // Two slots a scan, in pass order, each row once and none skipped (the
    // two inspections of one scan run side by side, in either order).
    let mut asked: Vec<usize> = seen
        .iter()
        .filter_map(|a| a.strip_prefix("claude/example/ask-")?.parse().ok())
        .collect();
    assert_eq!(asked.len(), 36, "{asked:?}");
    asked.sort_unstable();
    assert_eq!(asked, (0..36).collect::<Vec<_>>());
    assert!(!seen.iter().any(|a| a.contains("/idle-")));
}

#[test]
fn a_pass_ends_where_it_began_so_arrivals_cannot_hide_an_older_row() {
    // Review of #1191: an older session entering approval behind the pass,
    // while 16 new flagged rows arrive before every scan, was never found
    // again: every page stayed full and the pass never wrapped. A pass now
    // ends at the highest flagged row it started with.
    let untracked = named("untracked-0", "approval", false).row;
    assert!(zerus_desktop_catalog::approvals::approval_phase(&untracked));
    assert!(policy::candidate("", &zerus_client_core::view::row(&untracked)).is_none());
    let (run, seen) = inspections();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    let mut rows = idle(4_000);
    rows.push(named("older", "working", true));
    rows.extend((0..20).map(|n| named(&format!("untracked-{n}"), "approval", false)));
    f.source_rows(rows);
    f.second();
    let generation: i64 = f
        .store
        .db
        .query_row(
            "SELECT generation FROM desktop_catalog_rows WHERE key='run:run-older'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // The older session enters approval, keeping its row id.
    let older = named("older", "approval", true).row.to_string();
    f.store
        .db
        .execute(
            "UPDATE desktop_catalog_rows SET json=?1,bytes=length(?1),approval=1
             WHERE generation=?2 AND key='run:run-older'",
            params![older, generation],
        )
        .unwrap();
    let mut arrived = 20;
    for _ in 0..4 {
        // zerus-e04p: a loaded runner can spend more than FRESH between scans.
        f.runtime.sources.age_fixture(Duration::from_secs(16));
        for _ in 0..16 {
            let row = named(&format!("untracked-{arrived}"), "approval", false);
            let json = row.row.to_string();
            f.store
                .db
                .execute(
                    "INSERT INTO desktop_catalog_rows(generation,key,json,bytes,approval)
                     VALUES(?1,?2,?3,length(?3),1)",
                    params![generation, row.key, json],
                )
                .unwrap();
            arrived += 1;
        }
        f.second();
    }
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|a| a == "claude/example/older"), "{seen:?}");
}

#[test]
fn the_catalog_walk_still_gets_approval_slots_while_the_index_is_busy() {
    // The index and the walk take turns to go first: a row only the walk
    // sees (here, one whose flag is wrong) is inspected while 40 indexed
    // rows would otherwise keep both approval slots for 20 scans.
    let (run, seen) = inspections();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    let mut rows = vec![named("unflagged", "approval", true)];
    rows.extend((0..40).map(|n| named(&format!("ask-{n}"), "approval", true)));
    f.source_rows(rows);
    f.store
        .db
        .execute(
            "UPDATE desktop_catalog_rows SET approval=0 WHERE key='run:run-unflagged'",
            [],
        )
        .unwrap();
    for _ in 0..4 {
        f.second();
    }
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter().any(|a| a == "claude/example/unflagged"),
        "{seen:?}"
    );
}

#[test]
fn every_source_leads_the_index_in_turn_whatever_the_walk_visits() {
    // Review of #1191 R2: with two sources, the index's starting source and
    // the walk-first turns changed every scan in step, so one source's index
    // always came after its walk, which kept both slots with its first
    // manual-only questions as their cooldown ran out: a-10 was never
    // inspected. The index now moves to the next source only when it leads.
    let (run, seen) = inspections();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    let rows = |prefix: &str| -> Vec<CatalogItem> {
        (0..96)
            .map(|n| named(&format!("{prefix}-{n}"), "approval", true))
            .collect()
    };
    f.source_rows_on("", rows("a"));
    f.source_rows_on("peer", rows("b"));
    for _ in 0..60 {
        f.second();
    }
    let seen = seen.lock().unwrap();
    for name in ["claude/example/a-10", "claude/example/b-10"] {
        assert!(seen.iter().any(|a| a == name), "{name}: {seen:?}");
    }
}

/// Inspections that hold their approval slot until the test releases them:
/// each call waits for its own release, after the scans `hold` asks for.
struct Held {
    /// Each held call: the scan it started in, its sequence and release.
    calls: Mutex<Vec<(usize, usize, mpsc::Sender<()>)>>,
    names: Mutex<Vec<String>>,
    scan: AtomicUsize,
    sequence: AtomicUsize,
}
impl Held {
    fn runner() -> (Runner, Arc<Held>) {
        let held = Arc::new(Held {
            calls: Mutex::new(Vec::new()),
            names: Mutex::new(Vec::new()),
            scan: AtomicUsize::new(0),
            sequence: AtomicUsize::new(0),
        });
        let record = held.clone();
        let run: Runner = Arc::new(move |inv, _, cancel, _| {
            assert!(inv.args.iter().any(|a| a == "inspect"));
            if let Some(name) = inv.args.iter().find(|a| a.starts_with("claude/example/")) {
                record.names.lock().unwrap().push(name.clone());
            }
            let (release, wait) = mpsc::channel();
            let sequence = record.sequence.fetch_add(1, Ordering::SeqCst);
            record.calls.lock().unwrap().push((
                record.scan.load(Ordering::SeqCst),
                sequence,
                release,
            ));
            // This fixture holds by scan count, not wall time. Slow scheduling
            // must not release a slot early; panic/drop still cancels it.
            while !cancel.load(Ordering::Acquire) {
                match wait.recv_timeout(Duration::from_millis(10)) {
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                    _ => break,
                }
            }
            Exit::TimedOut
        });
        (run, held)
    }

    /// One-second scans: before each, the inspections held `hold(sequence)`
    /// scans return and are collected; the cooldown counts the second.
    fn scans(&self, f: &mut Fixture, count: usize, hold: impl Fn(usize) -> usize) {
        for scan in 1..=count {
            self.scan.store(scan, Ordering::SeqCst);
            self.calls
                .lock()
                .unwrap()
                .retain(|(at, sequence, release)| {
                    if scan - at >= hold(*sequence) {
                        let _ = release.send(());
                        false
                    } else {
                        true
                    }
                });
            let held = self.calls.lock().unwrap().len();
            f.collect_until(|r| r.jobs.len() == held);
            f.runtime.sources.heartbeat_fixture(&f.store);
            f.scan();
            // Every inspection the scan started has reached the runner.
            let launched = f.runtime.jobs.len();
            let until = Instant::now() + Duration::from_secs(5);
            while self.calls.lock().unwrap().len() < launched {
                assert!(Instant::now() < until, "a launched inspection never ran");
                thread::sleep(Duration::from_millis(1));
            }
            for (_, at) in f.runtime.recent.iter_mut() {
                *at = at.checked_sub(Duration::from_secs(1)).unwrap_or(*at);
            }
        }
        for (_, _, release) in self.calls.lock().unwrap().drain(..) {
            let _ = release.send(());
        }
        f.collect_until(|r| r.jobs.is_empty());
    }
}

fn slow_rows(f: &mut Fixture) {
    f.source_rows(
        (0..96)
            .map(|n| named(&format!("slow-{n}"), "approval", true))
            .collect(),
    );
}

#[test]
fn collecting_released_inspections_never_performs_a_scheduled_scan() {
    let (run, held) = Held::runner();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    slow_rows(&mut f);
    f.scan();
    let launched = f.runtime.jobs.len();
    assert_eq!(launched, APPROVALS);
    let until = Instant::now() + Duration::from_secs(5);
    while held.calls.lock().unwrap().len() < launched {
        assert!(Instant::now() < until, "the original inspections never ran");
        thread::sleep(Duration::from_millis(1));
    }
    for (_, _, release) in held.calls.lock().unwrap().drain(..) {
        release.send(()).unwrap();
    }
    // Make the interleaving exact: both original threads have returned before
    // the actor's next turn. The regression must not depend on a lucky race
    // between releasing a worker and its thread actually finishing.
    let until = Instant::now() + Duration::from_secs(5);
    while f.runtime.jobs.iter().any(|job| {
        job.thread
            .as_ref()
            .is_none_or(|thread| !thread.is_finished())
    }) {
        assert!(
            Instant::now() < until,
            "the original threads never finished"
        );
        thread::sleep(Duration::from_millis(1));
    }
    // The same scheduling point reached after a loaded runner spends one
    // second between explicit scans, without a machine-speed-dependent sleep.
    f.runtime.scan_at = Instant::now();
    f.collect_until(|r| r.jobs.is_empty());
    assert_eq!(held.sequence.load(Ordering::SeqCst), launched);
}

#[test]
fn slow_inspections_cannot_hand_every_admission_to_the_catalog_walk() {
    // Review of #1191 R3: inspections that hold both slots for four scans.
    // The turn flipped on every scan, busy or not, so each round of free
    // slots fell on a walk-first scan and the walk re-asked slow-0 to slow-3
    // as their cooldown ran out: slow-10 was never inspected. A turn now
    // passes only on a scan with a free slot.
    let (run, held) = Held::runner();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    slow_rows(&mut f);
    held.scans(&mut f, 40, |_| 4);
    let names = held.names.lock().unwrap();
    assert!(
        names.iter().any(|a| a == "claude/example/slow-10"),
        "{names:?}"
    );
}

#[test]
fn staggered_inspections_share_admission_between_the_index_and_the_walk() {
    // The two slots free on different scans (two and four scans held), but
    // always on scans of the same parity, which a per-scan turn gave the walk.
    let (run, held) = Held::runner();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    slow_rows(&mut f);
    held.scans(&mut f, 60, |call| if call % 2 == 0 { 2 } else { 4 });
    let names = held.names.lock().unwrap();
    assert!(
        names.iter().any(|a| a == "claude/example/slow-10"),
        "{names:?}"
    );
}

#[test]
fn with_slow_inspections_every_source_still_leads_the_index_in_turn() {
    // Leadership passes only on a scan that could admit, too: had busy
    // index-first scans moved it on, the four held scans of every round
    // would bring the same source back to lead each opportunity.
    let (run, held) = Held::runner();
    let mut f = Fixture::new(run);
    f.active();
    f.enabled(true);
    let rows = |prefix: &str| -> Vec<CatalogItem> {
        (0..96)
            .map(|n| named(&format!("{prefix}-{n}"), "approval", true))
            .collect()
    };
    f.source_rows_on("", rows("a"));
    f.source_rows_on("peer", rows("b"));
    held.scans(&mut f, 120, |_| 4);
    let names = held.names.lock().unwrap();
    for name in ["claude/example/a-10", "claude/example/b-10"] {
        assert!(names.iter().any(|a| a == name), "{name}: {names:?}");
    }
}
