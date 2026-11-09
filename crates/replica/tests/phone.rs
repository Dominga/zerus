//! The phone's task exchange: the computer's answer and the phone's side,
//! played against each other in one process as the relay carries them.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use zerus_replica::execution::EXECUTION_TASK_SCHEMA as EXECUTION;
use zerus_replica::phone::{
    answer, answer_with, Answer, Budget, Exchange, Inventories, Progress, Request,
};
use zerus_replica::{Actor, Inventory, NewTask, ProjectStore, SessionKey, Stage, Target, TaskDoc};

fn store(root: &Path) -> ProjectStore {
    ProjectStore::open(root, "project-1").unwrap()
}

fn add(store: &ProjectStore, text: &str) -> TaskDoc {
    let mut task = TaskDoc::create(NewTask {
        project: store.project_id(),
        id: &store.unused_short_id().unwrap(),
        text,
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap();
    store.save(&mut task).unwrap();
    task
}

/// Sends, reports and accepts `task`: it ends in Done.
fn accept(store: &ProjectStore, task: &mut TaskDoc) {
    let session = SessionKey {
        machine: "m".into(),
        session: "codex/api".into(),
        ..Default::default()
    };
    task.record_send(&session, "request-1", &Actor::Person, 2)
        .unwrap();
    task.record_report("Done.", &Actor::Agent(session.clone()), 3)
        .unwrap();
    task.set_stage(Stage::Done, &Actor::Person, 4).unwrap();
    store.save(task).unwrap();
}

fn request(value: serde_json::Value) -> Request {
    serde_json::from_value(value).unwrap()
}

fn refusal(store: &ProjectStore, value: serde_json::Value) -> String {
    answer(store, request(value)).unwrap_err().to_string()
}

#[test]
fn an_empty_phone_is_answered_the_live_tasks() {
    let root = tempfile::tempdir().unwrap();
    let computer = store(root.path());
    let live = [
        add(&computer, "Add rate limiting"),
        add(&computer, "Measure first"),
    ];
    let mut done = add(&computer, "Document the limits");
    accept(&computer, &mut done);
    let answered = answer(
        &computer,
        request(
            json!({"schema": 1, "project": "project-1", "inventory": {"project": {}, "tasks": {}}}),
        ),
    )
    .unwrap();
    let mut sent: Vec<_> = answered
        .updates
        .iter()
        .map(|u| u.task.clone().unwrap())
        .collect();
    sent.sort();
    let mut expected: Vec<_> = live.iter().map(|t| t.uuid().to_owned()).collect();
    expected.sort();
    assert_eq!(sent, expected);
    assert!(!answered.more);
    assert_eq!(
        answered.digest,
        computer.live_inventory().unwrap().0.digest()
    );
    assert_eq!(answered.inventory, None);
}

#[test]
fn the_answer_s_rules_refuse_what_hgs_refused() {
    let root = tempfile::tempdir().unwrap();
    let computer = store(root.path());
    let base =
        json!({"schema": 1, "project": "project-1", "inventory": {"project": {}, "tasks": {}}});
    let with = |key: &str, value: serde_json::Value| {
        let mut changed = base.clone();
        changed[key] = value;
        changed
    };
    const LOW: &str = "00000000-0000-4000-8000-000000000001";
    const HIGH: &str = "ffffffff-ffff-4fff-bfff-ffffffffffff";
    assert!(refusal(&computer, with("schema", json!(2))).contains("unknown exchange schema 2"));
    assert!(refusal(&computer, with("project", json!("project-2")))
        .contains("the request is for project \"project-2\""));
    assert!(refusal(
        &computer,
        with("range", json!({"after": HIGH, "through": LOW}))
    )
    .contains("an empty range"));
    assert!(
        refusal(&computer, with("range", json!({"after": "orchard"}))).contains("not a task UUID")
    );
    let mut outside = with("range", json!({"after": LOW}));
    outside["inventory"]["tasks"] = json!({LOW: {}});
    assert!(refusal(&computer, outside).contains("lies outside its range"));
    let many: Vec<_> = (0..257)
        .map(|_| json!({"task": uuid::Uuid::new_v4().to_string(), "bytes": ""}))
        .collect();
    assert!(refusal(&computer, with("updates", json!(many))).contains("at most 256 updates"));
    assert!(refusal(
        &computer,
        with("updates", json!([{"task": null, "bytes": "%%%"}]))
    )
    .contains("invalid base64 in an update"));
}

#[test]
fn a_range_answers_its_own_tasks() {
    let root = tempfile::tempdir().unwrap();
    let computer = store(root.path());
    let mut uuids: Vec<String> = (0..6)
        .map(|n| add(&computer, &format!("Task {n}")).uuid().to_owned())
        .collect();
    uuids.sort();
    let split = &uuids[2];
    let ask = |range: serde_json::Value| {
        let answered = answer(
            &computer,
            request(json!({"schema": 1, "project": "project-1",
                           "inventory": {"project": {}, "tasks": {}}, "range": range})),
        )
        .unwrap();
        let mut sent: Vec<_> = answered
            .updates
            .into_iter()
            .map(|u| u.task.unwrap())
            .collect();
        sent.sort();
        sent
    };
    assert_eq!(ask(json!({"through": split})), uuids[..3]);
    assert_eq!(ask(json!({"after": split})), uuids[3..]);
}

#[test]
fn the_wire_form_round_trips() {
    let wire = json!({"schema": 1, "project": "project-1",
        "inventory": {"project": {"12": 3}, "tasks": {}},
        "range": {"after": "00000000-0000-4000-8000-000000000001"},
        "updates": [{"task": null, "bytes": "bG9ybw=="}],
        "continue": {"task": null, "sha256": "ab".repeat(32), "offset": 7}});
    let parsed = request(wire.clone());
    assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
    // Absent optional fields stay absent.
    let bare =
        json!({"schema": 1, "project": "project-1", "inventory": {"project": {}, "tasks": {}}});
    assert_eq!(serde_json::to_value(request(bare.clone())).unwrap(), bare);
    // An answer with nothing for the phone names its inventory and no part.
    let root = tempfile::tempdir().unwrap();
    let computer = store(root.path());
    let answered = serde_json::to_value(answer(&computer, request(bare)).unwrap()).unwrap();
    assert_eq!(
        answered,
        json!({"schema": 1, "imported": 0, "refused": [], "updates": [], "more": false,
               "digest": Inventory::default().digest(), "inventory": {"project": {}, "tasks": {}}})
    );
}

/// Text that does not compress.
fn noise(seed: &str, length: usize) -> String {
    let mut out = String::new();
    while out.len() < length {
        out += &format!("{:x}", Sha256::digest(format!("{seed}-{}", out.len())));
    }
    out.truncate(length);
    out
}

fn comment(store: &ProjectStore, uuid: &str, text: &str) {
    let mut task = store.load(uuid).unwrap();
    task.comment(text, &Actor::Person, 5).unwrap();
    store.save(&mut task).unwrap();
}

fn comments(store: &ProjectStore, uuid: &str) -> usize {
    let task = store.load(uuid).unwrap();
    task.view()
        .unwrap()
        .log
        .iter()
        .filter(|e| e.kind == "comment")
        .count()
}

fn live_digest(store: &ProjectStore) -> String {
    store.live_inventory().unwrap().0.digest()
}

fn task_file(root: &Path, uuid: &str) -> PathBuf {
    root.join(format!("{:x}", Sha256::digest(b"project-1")))
        .join("tasks")
        .join(format!("{uuid}.loro"))
}

/// Every key, scalar, object and array, as the relay counts toward 50,000.
fn values(value: &Value) -> usize {
    match value {
        Value::Object(map) => 1 + map.values().map(|v| 1 + values(v)).sum::<usize>(),
        Value::Array(items) => 1 + items.iter().map(values).sum::<usize>(),
        _ => 1,
    }
}

/// A request or an answer as the relay carries it, within its bounds with
/// room for the envelope.
fn wire<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    let json = serde_json::to_value(value).unwrap();
    let size = serde_json::to_vec(&json).unwrap().len();
    assert!(size <= (1 << 20) - 4096, "{size} bytes");
    assert!(values(&json) <= 49_000, "{} values", values(&json));
    serde_json::from_value(json).unwrap()
}

#[derive(Default)]
struct Run {
    requests: usize,
    ranges: BTreeSet<String>,
    parts: usize,
    progress: Vec<Progress>,
    error: Option<String>,
}

/// One request and its answer; false once the exchange is done.
fn step(
    exchange: &mut Exchange,
    phone: &ProjectStore,
    computer: &ProjectStore,
    run: &mut Run,
) -> bool {
    let request = match exchange.next_request(phone) {
        Ok(Some(request)) => wire(&request),
        Ok(None) => return false,
        Err(error) => {
            run.error = Some(error.to_string());
            return false;
        }
    };
    run.requests += 1;
    run.ranges
        .insert(serde_json::to_string(&request.range).unwrap());
    let answered = wire(&answer(computer, request).unwrap());
    run.parts += usize::from(answered.part.is_some());
    match exchange.accept(phone, answered) {
        Ok(progress) => run.progress.push(progress),
        Err(error) => {
            run.error = Some(error.to_string());
            return false;
        }
    }
    true
}

fn run(phone: &ProjectStore, computer: &ProjectStore, budget: Budget) -> Run {
    let mut exchange = Exchange::with_budget(budget);
    let mut run = Run::default();
    while step(&mut exchange, phone, computer, &mut run) {}
    run
}

fn settled(run: Run) -> Run {
    assert_eq!(run.error, None);
    assert!(run.progress.last().is_some_and(|p| p.finished));
    run
}

fn pair() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    ProjectStore,
    ProjectStore,
) {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (phone, computer) = (store(a.path()), store(b.path()));
    (a, b, phone, computer)
}

#[test]
fn a_fresh_phone_gets_every_live_task() {
    let (_a, _b, phone, computer) = pair();
    for n in 0..30 {
        add(&computer, &format!("Task {n}"));
    }
    let mut done = add(&computer, "Document the limits");
    accept(&computer, &mut done);
    let pulled = settled(run(&phone, &computer, Budget::default()));
    assert_eq!(live_digest(&phone), live_digest(&computer));
    assert_eq!(
        pulled.progress.last().unwrap().digest,
        live_digest(&computer)
    );
    assert_eq!(phone.inventory().unwrap().0.tasks.len(), 30);
}

#[test]
fn the_phone_s_edits_reach_the_computer() {
    let (_a, _b, phone, computer) = pair();
    let task = add(&computer, "Add rate limiting");
    settled(run(&phone, &computer, Budget::default()));
    comment(&phone, task.uuid(), "From the phone");
    settled(run(&phone, &computer, Budget::default()));
    assert_eq!(comments(&computer, task.uuid()), 1);
    assert_eq!(live_digest(&phone), live_digest(&computer));
}

#[test]
fn both_sides_changed() {
    let (_a, _b, phone, computer) = pair();
    let task = add(&computer, "Add rate limiting");
    settled(run(&phone, &computer, Budget::default()));
    comment(&phone, task.uuid(), "From the phone");
    comment(&computer, task.uuid(), "From the computer");
    settled(run(&phone, &computer, Budget::default()));
    assert_eq!(comments(&phone, task.uuid()), 2);
    assert_eq!(comments(&computer, task.uuid()), 2);
    assert_eq!(live_digest(&phone), live_digest(&computer));
}

#[test]
fn a_held_task_follows_into_done() {
    let (_a, _b, phone, computer) = pair();
    let mut task = add(&computer, "Add rate limiting");
    settled(run(&phone, &computer, Budget::default()));
    accept(&computer, &mut task);
    settled(run(&phone, &computer, Budget::default()));
    assert_eq!(
        phone.load(task.uuid()).unwrap().stage().unwrap(),
        Stage::Done
    );
    assert_eq!(live_digest(&phone), live_digest(&computer));
}

#[test]
fn every_request_fits_the_relay() {
    let (_a, _b, phone, computer) = pair();
    // Every writer adds a peer to a task's version: 300 tasks of 40 writers
    // each pass both the inventory's bytes and its values budget.
    for n in 0..300 {
        let mut task = add(&computer, &format!("Task {n}"));
        for c in 0..39 {
            task = TaskDoc::from_bytes(&task.snapshot().unwrap()).unwrap();
            task.comment(&format!("Note {c}"), &Actor::Person, 5)
                .unwrap();
        }
        computer.save(&mut task).unwrap();
    }
    let peers = computer
        .inventory()
        .unwrap()
        .0
        .tasks
        .values()
        .next()
        .unwrap()
        .len();
    assert_eq!(peers, 40);
    let first = settled(run(&phone, &computer, Budget::default()));
    assert!(first.ranges.len() > 1, "{:?}", first.ranges);
    assert_eq!(live_digest(&phone), live_digest(&computer));
    // Nothing left: one request per range, and nothing changes.
    let again = settled(run(&phone, &computer, Budget::default()));
    assert_eq!(again.requests, again.ranges.len());
    assert!(again.progress.iter().all(|p| p.changed == 0));
}

#[test]
fn small_budgets_split_ranges_and_still_converge() {
    let (_a, _b, phone, computer) = pair();
    for n in 0..200 {
        add(&computer, &format!("Task {n}"));
    }
    let budget = Budget {
        inventory_bytes: 4096,
        ..Budget::default()
    };
    let first = settled(run(&phone, &computer, budget));
    assert!(first.ranges.len() > 3, "{:?}", first.ranges);
    assert_eq!(live_digest(&phone), live_digest(&computer));
    let task = phone
        .inventory()
        .unwrap()
        .0
        .tasks
        .keys()
        .nth(150)
        .unwrap()
        .clone();
    comment(&phone, &task, "From the phone");
    settled(run(&phone, &computer, budget));
    assert_eq!(comments(&computer, &task), 1);
}

#[test]
fn a_large_document_arrives_in_parts() {
    let (_a, _b, phone, computer) = pair();
    let task = add(&computer, "Add rate limiting");
    for n in 0..5 {
        comment(
            &computer,
            task.uuid(),
            &noise(&format!("comment-{n}"), 60_000),
        );
    }
    let done = settled(run(&phone, &computer, Budget::default()));
    assert!(done.parts >= 2, "{} parts", done.parts);
    assert_eq!(comments(&phone, task.uuid()), 5);
}

#[test]
fn a_part_out_of_step_starts_again() {
    let (_a, _b, phone, computer) = pair();
    let task = add(&computer, "Add rate limiting");
    for n in 0..5 {
        comment(
            &computer,
            task.uuid(),
            &noise(&format!("comment-{n}"), 60_000),
        );
    }
    let mut exchange = Exchange::new();
    let first = exchange.next_request(&phone).unwrap().unwrap();
    let answered = answer(&computer, first).unwrap();
    let mut forged = answered.part.clone().unwrap();
    exchange.accept(&phone, answered).unwrap();
    let asked = exchange.next_request(&phone).unwrap().unwrap();
    assert!(asked.resume.is_some());
    // A part that does not continue what arrived drops the document.
    forged.offset = 999;
    let out_of_step = Answer {
        schema: 1,
        imported: 0,
        refused: vec![],
        updates: vec![],
        part: Some(forged),
        more: true,
        digest: live_digest(&computer),
        inventory: None,
    };
    exchange.accept(&phone, out_of_step).unwrap();
    let afresh = exchange.next_request(&phone).unwrap().unwrap();
    assert_eq!(afresh.resume, None);
    // A complete document whose SHA-256 does not match is never imported, even
    // when its bytes would apply.
    let small = add(&computer, "Measure first");
    let bytes = computer.load(small.uuid()).unwrap().snapshot().unwrap();
    let mismatched = Answer {
        part: Some(zerus_replica::phone::Part {
            task: Some(small.uuid().to_owned()),
            size: bytes.len(),
            sha256: "00".repeat(32),
            offset: 0,
            bytes: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes),
        }),
        updates: vec![],
        ..answer(&computer, afresh).unwrap()
    };
    let progress = exchange.accept(&phone, mismatched).unwrap();
    assert_eq!(progress.changed, 0);
    assert!(phone.inventory().unwrap().0.tasks.is_empty());
    // The rest of the exchange brings it whole.
    let mut rest = Run::default();
    while step(&mut exchange, &phone, &computer, &mut rest) {}
    settled(rest);
    assert_eq!(comments(&phone, task.uuid()), 5);
    assert!(phone.load(small.uuid()).is_ok());
}

/// A task of another project, saved into `store`'s folder as a misplaced file would be.
fn misplaced(store: &ProjectStore) -> String {
    let mut task = TaskDoc::create(NewTask {
        project: "project-2",
        id: &store.unused_short_id().unwrap(),
        text: "Filed elsewhere",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap();
    store.save(&mut task).unwrap();
    task.uuid().to_owned()
}

#[test]
fn refused_documents_are_not_sent_again() {
    let (_a, b, phone, computer) = pair();
    let good = add(&computer, "Add rate limiting");
    settled(run(&phone, &computer, Budget::default()));
    // The computer's copy no longer loads: the phone's whole document replaces it.
    fs::write(task_file(b.path(), good.uuid()), b"damaged").unwrap();
    // The computer refuses a task of another project once, and it is not sent again.
    let stray = misplaced(&phone);
    let done = settled(run(&phone, &computer, Budget::default()));
    assert_eq!(comments(&computer, good.uuid()), 0);
    assert!(computer.load(good.uuid()).is_ok());
    let refused: Vec<_> = done
        .progress
        .iter()
        .flat_map(|p| p.refused_there.iter().map(|d| d.uuid.clone()))
        .collect();
    assert_eq!(refused, [stray]);
}

fn set_aside(root: &Path, uuid: &str) -> usize {
    fs::read_dir(task_file(root, uuid).parent().unwrap())
        .unwrap()
        .filter(|e| {
            let name = e.as_ref().unwrap().file_name();
            let name = name.to_string_lossy();
            name.starts_with(uuid) && name.contains(".damaged-")
        })
        .count()
}

/// zerus-cg5h.4 S2b.2 C1, review-n's control on #1409: an old phone's whole
/// pre-attempt document never replaces the computer's copy that records an
/// attempt. A damaged snapshot beside valid updates is rebuilt with every
/// attempt fact; the phone gets the execution schema back.
#[test]
fn an_old_phone_s_copy_keeps_the_computer_s_outstanding_attempt() {
    for damaged in [false, true] {
        let (_a, b, phone, computer) = pair();
        let task = add(&computer, "A phone still holds the pre-attempt copy");
        settled(run(&phone, &computer, Budget::default()));
        reserve_first_attempt(&computer, task.uuid());
        let expected = execution(&computer, task.uuid());
        comment(&phone, task.uuid(), "An ordinary offline comment");
        if damaged {
            let updates = task_file(b.path(), task.uuid()).with_extension("updates");
            assert!(updates.exists());
            fs::write(
                task_file(b.path(), task.uuid()),
                b"unreadable snapshot fixture",
            )
            .unwrap();
        }
        settled(run(&phone, &computer, Budget::default()));
        let current = computer.load(task.uuid()).unwrap();
        assert_eq!(
            execution(&computer, task.uuid()),
            expected,
            "damaged={damaged}"
        );
        assert!(current.held().unwrap().is_some());
        assert_eq!(current.schema(), EXECUTION);
        assert!(current.check_send().is_err());
        assert_eq!(comments(&computer, task.uuid()), 1);
        assert_eq!(
            set_aside(b.path(), task.uuid()),
            if damaged { 2 } else { 0 }
        );
        assert_eq!(phone.load(task.uuid()).unwrap().schema(), EXECUTION);
        assert_eq!(live_digest(&phone), live_digest(&computer));
    }
}

/// When nothing left here proves the phone's copy covers the lost snapshot,
/// the computer keeps its damaged copy for a person's review: nothing is set
/// aside, the task stays unloadable and closed, and the phone is told.
#[test]
fn a_damaged_copy_the_phone_cannot_cover_is_kept_for_review() {
    let (_a, b, phone, computer) = pair();
    let task = add(&computer, "The attempt only the computer's snapshot holds");
    settled(run(&phone, &computer, Budget::default()));
    reserve_first_attempt(&computer, task.uuid());
    // A cut append: the next save folds the attempt into the snapshot.
    let updates = task_file(b.path(), task.uuid()).with_extension("updates");
    let mut bytes = fs::read(&updates).unwrap();
    let appended = bytes.len();
    bytes.push(0);
    fs::write(&updates, bytes).unwrap();
    let mut folded = computer.load(task.uuid()).unwrap();
    computer.save(&mut folded).unwrap();
    // The fold leaves only the block of no changes that opens the file.
    assert!(fs::read(&updates).unwrap().len() < appended);
    comment(&phone, task.uuid(), "An ordinary offline comment");
    fs::write(
        task_file(b.path(), task.uuid()),
        b"unreadable snapshot fixture",
    )
    .unwrap();
    let done = settled(run(&phone, &computer, Budget::default()));
    let refused: Vec<_> = done
        .progress
        .iter()
        .flat_map(|p| p.refused_there.iter())
        .filter(|d| d.uuid == task.uuid())
        .collect();
    assert!(!refused.is_empty());
    assert!(
        refused[0].error.contains("person's review"),
        "{}",
        refused[0].error
    );
    // The phone is told it is kept for review, not ordinary damage.
    assert!(refused[0].kept);
    assert!(computer.load(task.uuid()).is_err());
    assert_eq!(
        fs::read(task_file(b.path(), task.uuid())).unwrap(),
        b"unreadable snapshot fixture"
    );
    assert_eq!(set_aside(b.path(), task.uuid()), 0);
    assert!(phone
        .load(task.uuid())
        .unwrap()
        .view()
        .unwrap()
        .execution
        .is_none());
    // The old phone's later edits are refused in each exchange, never queued
    // here for a later replay.
    comment(&phone, task.uuid(), "A later offline comment");
    let again = settled(run(&phone, &computer, Budget::default()));
    assert!(again
        .progress
        .iter()
        .flat_map(|p| p.refused_there.iter())
        .any(|d| d.uuid == task.uuid()));
    assert!(computer.load(task.uuid()).is_err());
    assert_eq!(set_aside(b.path(), task.uuid()), 0);
    // A person's review: moving the damaged files away still keeps it; only
    // removing the mark as well accepts the phone's copy.
    let tasks = task_file(b.path(), task.uuid())
        .parent()
        .unwrap()
        .to_owned();
    for (file, aside) in [
        ("loro", "reviewed-snapshot"),
        ("updates", "reviewed-updates"),
    ] {
        fs::rename(
            tasks.join(format!("{}.{file}", task.uuid())),
            tasks.join(aside),
        )
        .unwrap();
    }
    settled(run(&phone, &computer, Budget::default()));
    assert!(computer.load(task.uuid()).is_err());
    fs::remove_file(tasks.join(format!("{}.managed", task.uuid()))).unwrap();
    settled(run(&phone, &computer, Budget::default()));
    assert_eq!(comments(&computer, task.uuid()), 2);
    assert!(computer
        .load(task.uuid())
        .unwrap()
        .view()
        .unwrap()
        .execution
        .is_none());
}

#[test]
fn an_answer_that_only_repeats_a_refusal_ends_the_exchange() {
    let (_a, _b, phone, computer) = pair();
    add(&computer, "Add rate limiting");
    misplaced(&computer);
    let ended = run(&phone, &computer, Budget::default());
    let error = ended.error.unwrap();
    assert!(error.contains("do not apply here"), "{error}");
    assert!(ended.requests < 6, "{} requests", ended.requests);
}

#[test]
fn a_document_too_large_to_send_is_named_and_skipped() {
    let (_a, _b, phone, computer) = pair();
    let task = add(&computer, "Add rate limiting");
    let small = add(&computer, "Measure first");
    settled(run(&phone, &computer, Budget::default()));
    comment(&phone, task.uuid(), &noise("a", 50_000));
    comment(&phone, task.uuid(), &noise("b", 50_000));
    comment(&phone, small.uuid(), "Short");
    let budget = Budget {
        push_bytes: 64 * 1024,
        ..Budget::default()
    };
    let done = settled(run(&phone, &computer, budget));
    let unsent: BTreeSet<_> = done
        .progress
        .iter()
        .flat_map(|p| p.unsent.clone())
        .collect();
    assert_eq!(unsent, BTreeSet::from([task.uuid().to_owned()]));
    assert_eq!(comments(&computer, small.uuid()), 1);
    assert_eq!(comments(&computer, task.uuid()), 0);
}

#[test]
fn a_document_that_leaves_while_in_parts_ends_the_exchange() {
    let (_a, _b, phone, computer) = pair();
    let task = add(&computer, "Add rate limiting");
    for n in 0..5 {
        comment(
            &computer,
            task.uuid(),
            &noise(&format!("comment-{n}"), 60_000),
        );
    }
    let mut exchange = Exchange::new();
    let mut run = Run::default();
    assert!(step(&mut exchange, &phone, &computer, &mut run));
    assert_eq!(run.parts, 1);
    // Accepted into Done before its last part: the phone never held it, so the
    // computer no longer offers it.
    let mut fresh = computer.load(task.uuid()).unwrap();
    accept(&computer, &mut fresh);
    while step(&mut exchange, &phone, &computer, &mut run) {}
    let run = settled(run);
    assert!(run.requests < 5, "{} requests", run.requests);
    assert!(phone.inventory().unwrap().0.tasks.is_empty());
}

fn empty_answer(computer: &ProjectStore) -> Answer {
    Answer {
        schema: 1,
        imported: 0,
        refused: vec![],
        updates: vec![],
        part: None,
        more: false,
        digest: live_digest(computer),
        inventory: None,
    }
}

#[test]
fn a_part_of_a_document_larger_than_any_task_is_refused() {
    let (_a, _b, phone, computer) = pair();
    let mut exchange = Exchange::new();
    exchange.next_request(&phone).unwrap().unwrap();
    let huge = Answer {
        part: Some(zerus_replica::phone::Part {
            task: Some("0b9d6c55-2f5e-4c39-9a51-0d6f3c2a7e11".into()),
            size: 2
                * (zerus_replica::MAX_DOCUMENT
                    + zerus_replica::MAX_ATTACHMENT_TOTAL.div_ceil(3) * 4)
                + 1,
            sha256: "00".repeat(32),
            offset: 0,
            bytes: "bG9ybw==".into(),
        }),
        more: true,
        ..empty_answer(&computer)
    };
    let error = exchange.accept(&phone, huge).unwrap_err().to_string();
    assert!(error.contains("too large"), "{error}");
}

#[test]
fn task_media_survives_paged_phone_replication_and_reopening() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let (_a, _b, phone, computer) = pair();
    let mut task = add(&computer, "Inspect [Image #1]");
    // Incompressible enough to exercise actual update parts, beyond the old
    // text-only document assembly ceiling.
    let mut bytes = Vec::new();
    for i in 0u64..280000 {
        bytes.extend_from_slice(&Sha256::digest(i.to_le_bytes()));
    }
    let attachment = zerus_replica::TaskAttachment {
        name: "capture.png".into(),
        mime: "image/png".into(),
        data_base64: STANDARD.encode(&bytes),
        reference: "[Image #1]".into(),
    };
    task.set_attachments(std::slice::from_ref(&attachment), &Actor::Person, 2)
        .unwrap();
    computer.save(&mut task).unwrap();
    let result = settled(run(&phone, &computer, Budget::default()));
    assert!(result.parts > 1);
    let loaded = phone.load(task.uuid()).unwrap();
    assert_eq!(loaded.attachments().unwrap(), [attachment]);
    assert_eq!(loaded.attachments().unwrap()[0].bytes().unwrap(), bytes);
    assert_eq!(live_digest(&phone), live_digest(&computer));
}

#[test]
fn an_answer_that_promises_more_and_brings_nothing_ends_the_exchange() {
    let (_a, _b, phone, computer) = pair();
    add(&computer, "Add rate limiting");
    let mut exchange = Exchange::new();
    exchange.next_request(&phone).unwrap().unwrap();
    let empty = Answer {
        more: true,
        ..empty_answer(&computer)
    };
    let error = exchange.accept(&phone, empty).unwrap_err().to_string();
    assert!(error.contains("brought nothing"), "{error}");
    assert!(exchange.finished());
}

#[test]
fn a_done_task_the_phone_asks_for_arrives_and_stays() {
    let (_a, _b, phone, computer) = pair();
    add(&computer, "Add rate limiting");
    let mut done = add(&computer, "Document the limits");
    accept(&computer, &mut done);
    settled(run(&phone, &computer, Budget::default()));
    assert!(phone.load(done.uuid()).is_err());
    // Opening it on the phone fetches it whole; later exchanges keep it.
    let mut exchange = Exchange::with_budget(Budget::default());
    exchange.fetch([done.uuid().to_owned()]);
    let mut fetched = Run::default();
    while step(&mut exchange, &phone, &computer, &mut fetched) {}
    settled(fetched);
    assert_eq!(
        phone.load(done.uuid()).unwrap().stage().unwrap(),
        Stage::Done
    );
    comment(&computer, done.uuid(), "Later on the computer");
    settled(run(&phone, &computer, Budget::default()));
    assert_eq!(comments(&phone, done.uuid()), 1);
    // A task the computer does not have ends the exchange all the same.
    let mut unknown = Exchange::new();
    unknown.fetch(["00000000-0000-4000-8000-000000000009".to_owned()]);
    let mut nothing = Run::default();
    while step(&mut unknown, &phone, &computer, &mut nothing) {}
    settled(nothing);
}

/// The computer reserves `task`'s first attempt, so it records attempts.
fn reserve_first_attempt(computer: &ProjectStore, uuid: &str) {
    use zerus_replica::execution::{NativeRequest, Origin};
    let executor = SessionKey {
        machine: "m".into(),
        session: "codex/api".into(),
        session_id: "launch_id:api".into(),
    };
    let mut task = computer.load(uuid).unwrap();
    task.reserve_attempt(
        "11111111-1111-4111-8111-111111111111",
        Origin::Send,
        &executor,
        NativeRequest {
            machine: "m".into(),
            name: "codex/api".into(),
            run_id: "run-api".into(),
            conversation_id: "conversation-api".into(),
            digest: "ab".repeat(32),
        },
        &Actor::Person,
        10,
    )
    .unwrap();
    computer.save(&mut task).unwrap();
}

fn execution(store: &ProjectStore, uuid: &str) -> String {
    serde_json::to_string(&store.load(uuid).unwrap().view().unwrap().execution).unwrap()
}

/// zerus-cg5h.4 S2b.2 leaf 3: a phone's offline edit concurrent with the
/// first attempt comes back with the execution schema, and the phone's
/// payload adds no attempt fact (phones have no attempt API).
#[test]
fn a_phone_edit_beside_the_first_attempt_comes_back_at_the_execution_schema() {
    let (_a, _b, phone, computer) = pair();
    let task = add(&computer, "Add rate limiting");
    settled(run(&phone, &computer, Budget::default()));
    reserve_first_attempt(&computer, task.uuid());
    let facts = execution(&computer, task.uuid());
    comment(&phone, task.uuid(), "Offline on the phone");
    settled(run(&phone, &computer, Budget::default()));
    for side in [&phone, &computer] {
        let copy = side.load(task.uuid()).unwrap();
        assert_eq!(copy.schema(), EXECUTION);
        assert!(!copy.needs_schema_repair());
        assert!(copy.held().unwrap().is_some());
    }
    assert_eq!(comments(&computer, task.uuid()), 1);
    assert_eq!(execution(&computer, task.uuid()), facts);
    assert_eq!(live_digest(&phone), live_digest(&computer));
}

/// The computer imports the phone's edit and answers, but the answer is
/// lost; the phone edits again offline and reconnects.
#[test]
fn a_lost_answer_and_a_reconnect_still_bring_the_execution_schema() {
    let (_a, _b, phone, computer) = pair();
    let task = add(&computer, "Add rate limiting");
    settled(run(&phone, &computer, Budget::default()));
    reserve_first_attempt(&computer, task.uuid());
    comment(&phone, task.uuid(), "Before the lost answer");
    let mut exchange = Exchange::with_budget(Budget::default());
    let request = exchange.next_request(&phone).unwrap().unwrap();
    let _lost: Answer = answer(&computer, wire(&request)).unwrap();
    comment(&phone, task.uuid(), "After it");
    settled(run(&phone, &computer, Budget::default()));
    let copy = phone.load(task.uuid()).unwrap();
    assert!(!copy.needs_schema_repair() && copy.schema() == EXECUTION);
    assert_eq!(comments(&computer, task.uuid()), 2);
    assert_eq!(live_digest(&phone), live_digest(&computer));
}

/// Several ranges and pages: every held task reaches the phone with the
/// execution schema, the others keep theirs.
#[test]
fn held_tasks_arrive_at_the_execution_schema_across_pages() {
    let (_a, _b, phone, computer) = pair();
    let tasks: Vec<TaskDoc> = (0..12)
        .map(|n| add(&computer, &noise(&format!("task {n}"), 40_000)))
        .collect();
    settled(run(&phone, &computer, Budget::default()));
    for task in [&tasks[2], &tasks[9]] {
        reserve_first_attempt(&computer, task.uuid());
    }
    for task in &tasks {
        comment(&phone, task.uuid(), "Offline");
    }
    let small = Budget {
        inventory_bytes: 1024,
        push_bytes: 96 * 1024,
        ..Budget::default()
    };
    let run = settled(run(&phone, &computer, small));
    assert!(
        run.requests > 1 && run.ranges.len() > 1,
        "{} requests",
        run.requests
    );
    for (n, task) in tasks.iter().enumerate() {
        let copy = phone.load(task.uuid()).unwrap();
        assert_eq!(copy.schema() == EXECUTION, n == 2 || n == 9, "task {n}");
        assert!(!copy.needs_schema_repair());
    }
    assert_eq!(live_digest(&phone), live_digest(&computer));
}

/// Inventories from the store itself, recording what each call saw.
struct Recorded<'a> {
    store: &'a ProjectStore,
    calls: Vec<String>,
}

impl Inventories for Recorded<'_> {
    fn phone(&mut self, theirs: &Inventory) -> zerus_replica::Result<Inventory> {
        let ours = self.store.phone_inventory(theirs)?;
        self.calls.push(format!("phone {}", ours.tasks.len()));
        Ok(ours)
    }

    fn live_digest(&mut self) -> zerus_replica::Result<String> {
        self.calls.push("live".into());
        Ok(self.store.live_inventory()?.0.digest())
    }
}

#[test]
fn an_answer_reads_its_inventories_from_the_source_it_is_given_after_the_import() {
    let (phone_root, computer_root) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (phone, computer) = (store(phone_root.path()), store(computer_root.path()));
    add(&computer, "Add rate limiting");
    let mut done = add(&computer, "Document the limits");
    accept(&computer, &mut done);
    // The phone brings a task the computer does not hold yet.
    let new = add(&phone, "Written on the phone");
    let bytes = phone.load(new.uuid()).unwrap().snapshot().unwrap();
    let ask = json!({"schema": 1, "project": "project-1", "inventory": {"project": {}, "tasks": {}},
        "updates": [{"task": new.uuid(), "bytes": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)}]});
    let mut recorded = Recorded {
        store: &computer,
        calls: Vec::new(),
    };
    let given = answer_with(&computer, request(ask.clone()), &mut recorded).unwrap();
    // Asked once each, after the phone's task was imported: two live tasks.
    assert_eq!(recorded.calls, ["phone 2", "live"]);
    assert_eq!(given.imported, 1);
    // The store's own inventories give the same answer (the task is already here now).
    let again = answer(&computer, request(ask)).unwrap();
    assert_eq!(again.imported, 0);
    assert_eq!(
        Answer {
            imported: 1,
            ..again
        },
        given
    );
}

fn raw_task(bytes: &[u8]) -> loro::LoroDoc {
    let raw = loro::LoroDoc::new();
    raw.import(bytes).unwrap();
    raw
}

fn update_bytes(answer: &Answer, uuid: &str) -> Vec<u8> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD
        .decode(
            &answer
                .updates
                .iter()
                .find(|u| u.task.as_deref() == Some(uuid))
                .unwrap()
                .bytes,
        )
        .unwrap()
}

fn fetch_done(phone: &ProjectStore, computer: &ProjectStore, uuid: &str) {
    let mut exchange = Exchange::new();
    exchange.fetch([uuid.to_owned()]);
    let mut run = Run::default();
    while step(&mut exchange, phone, computer, &mut run) {}
    settled(run);
}

#[test]
fn a_done_fetch_is_shallow_but_keeps_the_complete_task_state_and_attempt_facts() {
    use zerus_replica::execution::{Receipt, ReportKind};
    let (_a, _b, phone, computer) = pair();
    let mut task = add(&computer, "Investigate and repair task replication");
    // A long-lived requirement: many revisions, discussion, then one exact
    // execution. The visible log stays; superseded text operations do not.
    for revision in 1..=120 {
        let sections: String = (0..24).map(|n| format!(
            "Section {n}: review protocol revision {revision}, preserve request identity {}, and verify bounded offline recovery.\n",
            noise(&format!("revision-{revision}-section-{n}"), 32)
        )).collect();
        task.edit_text(
            &format!("# Task replication, revision {revision}\n{sections}"),
            &Actor::Person,
            revision + 1,
        )
        .unwrap();
        if revision % 3 == 0 {
            task.comment(
                &format!("Revision {revision} reviewed; preserve reports and attempt evidence."),
                &Actor::Person,
                revision + 2,
            )
            .unwrap();
        }
    }
    task.floor().unwrap();
    computer.save(&mut task).unwrap();
    reserve_first_attempt(&computer, task.uuid());
    let mut task = computer.load(task.uuid()).unwrap();
    let id = "11111111-1111-4111-8111-111111111111";
    let executor = task.attempt(id).unwrap().unwrap().executor;
    let receipt = Receipt {
        request_id: id.into(),
        name: "codex/api".into(),
        run_id: "run-api".into(),
        conversation_id: "conversation-api".into(),
        first_message: false,
        digest: "ab".repeat(32),
        submitted: true,
    };
    task.settle_attempt(
        id,
        zerus_replica::Settlement::Receipt(&receipt),
        &Actor::Person,
        200,
    )
    .unwrap();
    task.record_start(&Actor::Agent(executor.clone()), 201)
        .unwrap();
    assert!(
        task.report_attempt(
            id,
            "Ready for review.",
            ReportKind::Ready,
            None,
            &Actor::Agent(executor),
            202
        )
        .unwrap()
        .moves
    );
    task.set_stage(Stage::Done, &Actor::Person, 203).unwrap();
    computer.save(&mut task).unwrap();
    assert!(task.floor_record().is_some());
    let full = task.snapshot().unwrap();
    let answered = answer(&computer, request(json!({"schema":1,"project":"project-1","inventory":{"project":{},"tasks":{task.uuid():{}}}}))).unwrap();
    let shallow = update_bytes(&answered, task.uuid());
    let raw = raw_task(&shallow);
    assert!(raw.is_shallow());
    assert_eq!(raw.get_deep_value(), raw_task(&full).get_deep_value());
    assert_eq!(raw.oplog_vv(), raw_task(&full).oplog_vv());
    assert!(
        shallow.len() < full.len(),
        "full {}, shallow {}",
        full.len(),
        shallow.len()
    );
    println!(
        "120 requirement revisions, 40 comments, one completed attempt: full={} shallow={}",
        full.len(),
        shallow.len()
    );
    let value = serde_json::to_value(&answered).unwrap();
    assert_eq!(value["schema"], 1);
    for required in ["schema", "imported", "refused", "updates", "more", "digest"] {
        assert!(value.get(required).is_some());
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(required);
        assert!(
            serde_json::from_value::<Answer>(missing).is_err(),
            "missing {required}"
        );
    }
    assert_eq!(value.as_object().unwrap().len(), 6);
    fetch_done(&phone, &computer, task.uuid());
    let loaded = phone.load(task.uuid()).unwrap();
    assert_eq!(loaded.floor_record(), task.floor_record());
    assert_eq!(
        loaded.attempt_facts(id).unwrap(),
        task.attempt_facts(id).unwrap()
    );
    assert_eq!(
        serde_json::to_value(loaded.view().unwrap()).unwrap(),
        serde_json::to_value(task.view().unwrap()).unwrap()
    );
}

#[test]
fn only_an_empty_version_done_fetch_changes_its_encoding() {
    let (_a, _b, phone, computer) = pair();
    let mut task = add(&computer, "Retain the ordinary live and desktop exchange");
    let empty = || {
        request(
            json!({"schema":1,"project":"project-1","inventory":{"project":{},"tasks":{task.uuid():{}}}}),
        )
    };
    assert!(!raw_task(&update_bytes(
        &answer(&computer, empty()).unwrap(),
        task.uuid()
    ))
    .is_shallow());
    accept(&computer, &mut task);
    // Ordinary desktop initial replication still carries the full document.
    let (ours, _) = computer.inventory().unwrap();
    let (full, _) = computer
        .updates_for(&ours, &Inventory::default(), usize::MAX)
        .unwrap();
    assert!(!raw_task(
        &full
            .iter()
            .find(|u| matches!(&u.document,zerus_replica::Document::Task(id) if id==task.uuid()))
            .unwrap()
            .bytes
    )
    .is_shallow());
    // A cached full copy requests a nonempty version, and keeps that copy.
    let imported = phone.import_updates(full).unwrap();
    assert!(imported.refused.is_empty());
    let before = phone.load(task.uuid()).unwrap();
    comment(
        &computer,
        task.uuid(),
        "A later comment uses the existing delta",
    );
    let request = Request {
        schema: 1,
        project: "project-1".into(),
        inventory: phone.inventory().unwrap().0,
        range: None,
        updates: vec![],
        resume: None,
    };
    let answer = answer(&computer, request).unwrap();
    let bytes = update_bytes(&answer, task.uuid());
    assert_eq!(
        bytes,
        computer
            .load(task.uuid())
            .unwrap()
            .export_since(&before.version())
            .unwrap()
    );
    settled(run(&phone, &computer, Budget::default()));
    assert!(!raw_task(&phone.load(task.uuid()).unwrap().snapshot().unwrap()).is_shallow());
}

#[test]
fn a_shallow_done_copy_keeps_later_comments_and_a_desktop_reopen() {
    let (root, _b, phone, computer) = pair();
    let mut task = add(&computer, "Continue after opening Done on the phone");
    accept(&computer, &mut task);
    fetch_done(&phone, &computer, task.uuid());
    let path = task_file(root.path(), task.uuid());
    let bytes = fs::read(&path).unwrap();
    // Merely opening the cached Done task again never rewrites its files.
    fetch_done(&phone, &computer, task.uuid());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    comment(&computer, task.uuid(), "Later on the computer");
    settled(run(&phone, &computer, Budget::default()));
    comment(&phone, task.uuid(), "Later on the phone");
    settled(run(&phone, &computer, Budget::default()));
    let mut reopened = computer.load(task.uuid()).unwrap();
    reopened.set_stage(Stage::Todo, &Actor::Person, 8).unwrap();
    computer.save(&mut reopened).unwrap();
    settled(run(&phone, &computer, Budget::default()));
    assert_eq!(
        phone.load(task.uuid()).unwrap().stage().unwrap(),
        Stage::Todo
    );
    assert_eq!(comments(&phone, task.uuid()), 2);
    assert_eq!(
        phone.inventory().unwrap().0,
        computer.inventory().unwrap().0
    );
}

fn pre_root_pair(phone: &ProjectStore, computer: &ProjectStore) -> String {
    let mut task = add(computer, "Original task text before accepting it");
    let mut offline = TaskDoc::from_bytes(&task.snapshot().unwrap()).unwrap();
    task.edit_text("Entirely different final requirement", &Actor::Person, 2)
        .unwrap();
    accept(computer, &mut task);
    // Seed the independently specified shallow form, so this recovery
    // control does not depend on the candidate answerer's encoding choice.
    let raw = raw_task(&task.snapshot().unwrap());
    let bytes = raw
        .export(loro::ExportMode::shallow_snapshot(&raw.oplog_frontiers()))
        .unwrap();
    let imported = phone
        .import_updates(vec![zerus_replica::Update {
            document: zerus_replica::Document::Task(task.uuid().into()),
            bytes,
        }])
        .unwrap();
    assert!(imported.refused.is_empty());
    // Another computer still holds the earlier branch. Its edit depends on
    // characters the phone's new shallow root no longer retains.
    offline
        .edit_text(
            "Original task text with an offline change",
            &Actor::Person,
            5,
        )
        .unwrap();
    let delta = offline.export_since(&task.version()).unwrap();
    let imported = computer
        .import_updates(vec![zerus_replica::Update {
            document: zerus_replica::Document::Task(task.uuid().into()),
            bytes: delta,
        }])
        .unwrap();
    assert!(imported.refused.is_empty());
    task.uuid().into()
}

fn refuse_then_request_fresh(
    exchange: &mut Exchange,
    phone: &ProjectStore,
    computer: &ProjectStore,
    uuid: &str,
) -> Request {
    let before = phone.load(uuid).unwrap().version();
    let request = exchange.next_request(phone).unwrap().unwrap();
    assert!(!request.inventory.tasks[uuid].is_empty());
    let answered = answer(computer, request).unwrap();
    let progress = exchange.accept(phone, answered).unwrap();
    assert_eq!(progress.refused_here.len(), 1);
    assert!(progress.refused_here[0].error.contains("shallow history"));
    assert_eq!(phone.load(uuid).unwrap().version(), before);
    let fresh = exchange.next_request(phone).unwrap().unwrap();
    assert!(fresh.inventory.tasks[uuid].is_empty());
    assert!(fresh
        .updates
        .iter()
        .all(|u| u.task.as_deref() != Some(uuid)));
    fresh
}

#[test]
fn pre_root_concurrency_refuses_then_refreshes_only_a_fully_covered_copy() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    for legacy_full_answer in [false, true] {
        let (root, _b, phone, computer) = pair();
        let uuid = pre_root_pair(&phone, &computer);
        let path = task_file(root.path(), &uuid);
        let saved = fs::read(&path).unwrap();
        let mut exchange = Exchange::new();
        let fresh = refuse_then_request_fresh(&mut exchange, &phone, &computer, &uuid);
        assert_eq!(
            fs::read(&path).unwrap(),
            saved,
            "refusal never clears the copy"
        );
        let mut reply = answer(&computer, fresh).unwrap();
        if legacy_full_answer {
            reply
                .updates
                .iter_mut()
                .find(|u| u.task.as_deref() == Some(&uuid))
                .unwrap()
                .bytes = STANDARD.encode(computer.load(&uuid).unwrap().snapshot().unwrap());
        }
        exchange.accept(&phone, reply).unwrap();
        let mut rest = Run::default();
        while step(&mut exchange, &phone, &computer, &mut rest) {}
        settled(rest);
        assert_eq!(
            phone.load(&uuid).unwrap().version(),
            computer.load(&uuid).unwrap().version()
        );
        assert_eq!(
            serde_json::to_value(phone.load(&uuid).unwrap().view().unwrap()).unwrap(),
            serde_json::to_value(computer.load(&uuid).unwrap().view().unwrap()).unwrap()
        );
    }
}

#[test]
fn pre_root_refresh_keeps_unsent_edits_including_a_writer_after_the_request() {
    for edit_after_request in [false, true] {
        let (root, _b, phone, computer) = pair();
        let uuid = pre_root_pair(&phone, &computer);
        if !edit_after_request {
            comment(&phone, &uuid, "Unsent local edit");
        }
        let mut exchange = Exchange::new();
        let fresh = refuse_then_request_fresh(&mut exchange, &phone, &computer, &uuid);
        let reply = answer(&computer, fresh).unwrap();
        if edit_after_request {
            comment(&phone, &uuid, "A writer after the fresh request");
        }
        let before = phone.load(&uuid).unwrap().version();
        let path = task_file(root.path(), &uuid);
        let saved = fs::read(&path).unwrap();
        let error = exchange.accept(&phone, reply).unwrap_err().to_string();
        assert!(
            error.contains("unsent edits") && error.contains("review"),
            "{error}"
        );
        assert_eq!(phone.load(&uuid).unwrap().version(), before);
        assert_eq!(fs::read(path).unwrap(), saved);
        assert_eq!(comments(&phone, &uuid), 1);
        assert_eq!(comments(&computer, &uuid), 0);
    }
}

#[test]
fn a_shallow_done_fetch_uses_the_existing_multipart_resume_and_restart() {
    let (_a, _b, phone, computer) = pair();
    let mut task = add(&computer, "Inspect a large completed discussion");
    for n in 0..5 {
        task.comment(
            &noise(&format!("shallow-comment-{n}"), 60_000),
            &Actor::Person,
            n + 2,
        )
        .unwrap();
    }
    use base64::{engine::general_purpose::STANDARD, Engine};
    let data: Vec<u8> = (0u64..24_000)
        .flat_map(|i| Sha256::digest(i.to_le_bytes()))
        .collect();
    let attachment = zerus_replica::TaskAttachment {
        name: "capture.png".into(),
        mime: "image/png".into(),
        data_base64: STANDARD.encode(&data),
        reference: "[Image #1]".into(),
    };
    task.set_attachments(std::slice::from_ref(&attachment), &Actor::Person, 8)
        .unwrap();
    accept(&computer, &mut task);
    let mut exchange = Exchange::new();
    exchange.fetch([task.uuid().into()]);
    let first = exchange.next_request(&phone).unwrap().unwrap();
    let reply = answer(&computer, first).unwrap();
    let part = reply
        .part
        .as_ref()
        .expect("large shallow state still uses parts");
    assert_eq!(part.offset, 0);
    let first_hash = part.sha256.clone();
    exchange.accept(&phone, reply).unwrap();
    let next = exchange.next_request(&phone).unwrap().unwrap();
    assert_eq!(next.resume.as_ref().unwrap().sha256, first_hash);
    let continued = answer(&computer, next.clone()).unwrap();
    assert!(continued.part.as_ref().unwrap().offset > 0);
    // A changed document restarts the same bounded part protocol at zero.
    comment(
        &computer,
        task.uuid(),
        "Changed while the old parts were in flight",
    );
    let restarted = answer(&computer, next).unwrap();
    assert_eq!(restarted.part.as_ref().unwrap().offset, 0);
    assert_ne!(restarted.part.as_ref().unwrap().sha256, first_hash);
    exchange.accept(&phone, restarted).unwrap();
    let mut rest = Run::default();
    while step(&mut exchange, &phone, &computer, &mut rest) {}
    settled(rest);
    assert_eq!(comments(&phone, task.uuid()), 6);
    assert_eq!(
        phone.load(task.uuid()).unwrap().attachments().unwrap(),
        [attachment]
    );
    assert!(raw_task(&phone.load(task.uuid()).unwrap().snapshot().unwrap()).is_shallow());
}

#[test]
fn a_fresh_fetch_never_replaces_a_copy_that_became_damaged() {
    let (root, _b, phone, computer) = pair();
    let uuid = pre_root_pair(&phone, &computer);
    let mut exchange = Exchange::new();
    let fresh = refuse_then_request_fresh(&mut exchange, &phone, &computer, &uuid);
    let reply = answer(&computer, fresh).unwrap();
    let path = task_file(root.path(), &uuid);
    fs::write(&path, b"damaged after the request").unwrap();
    let error = exchange.accept(&phone, reply).unwrap_err().to_string();
    assert!(!error.is_empty());
    assert_eq!(fs::read(path).unwrap(), b"damaged after the request");
}

#[test]
fn a_shallow_snapshot_does_not_lower_an_unknown_task_schema_or_enable_writes() {
    let (_a, _b, phone, computer) = pair();
    let mut task = add(&computer, "A task from a newer app");
    accept(&computer, &mut task);
    let raw = raw_task(&task.snapshot().unwrap());
    raw.get_map("meta").insert("schema", 999).unwrap();
    let mut task = TaskDoc::from_bytes(&raw.export(loro::ExportMode::Snapshot).unwrap()).unwrap();
    computer.save(&mut task).unwrap();
    fetch_done(&phone, &computer, task.uuid());
    let mut loaded = phone.load(task.uuid()).unwrap();
    assert_eq!(loaded.stored_schema(), 999);
    assert!(raw_task(&loaded.snapshot().unwrap()).is_shallow());
    assert!(loaded
        .comment("Must stay read-only", &Actor::Person, 8)
        .is_err());
}

#[test]
fn a_phone_keeps_accepting_done_snapshots_from_an_older_full_answerer() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let (_a, _b, phone, computer) = pair();
    let mut task = add(&computer, "A Done task from an older answerer");
    accept(&computer, &mut task);
    let mut exchange = Exchange::new();
    exchange.fetch([task.uuid().into()]);
    let requested = exchange.next_request(&phone).unwrap().unwrap();
    assert!(requested.inventory.tasks[task.uuid()].is_empty());
    let legacy:Answer=serde_json::from_value(json!({"schema":1,"imported":0,"refused":[],"updates":[{"task":task.uuid(),"bytes":STANDARD.encode(task.snapshot().unwrap())}],"more":false,"digest":live_digest(&computer)})).unwrap();
    exchange.accept(&phone, legacy).unwrap();
    let mut rest = Run::default();
    while step(&mut exchange, &phone, &computer, &mut rest) {}
    settled(rest);
    let loaded = phone.load(task.uuid()).unwrap();
    assert!(!raw_task(&loaded.snapshot().unwrap()).is_shallow());
    assert_eq!(
        serde_json::to_value(loaded.view().unwrap()).unwrap(),
        serde_json::to_value(task.view().unwrap()).unwrap()
    );
}

#[test]
fn review_a_fresh_fetch_keeps_a_corrupted_unsent_update_block_for_review() {
    let (root, _b, phone, computer) = pair();
    let uuid = pre_root_pair(&phone, &computer);
    let mut exchange = Exchange::new();
    let fresh = refuse_then_request_fresh(&mut exchange, &phone, &computer, &uuid);
    let reply = answer(&computer, fresh).unwrap();
    comment(
        &phone,
        &uuid,
        "Unsent comment whose bytes must be kept for review",
    );
    assert_eq!(comments(&phone, &uuid), 1);
    let snapshot = task_file(root.path(), &uuid);
    let updates = snapshot.with_extension("updates");
    let saved_snapshot = fs::read(&snapshot).unwrap();
    let mut damaged_update = fs::read(&updates).unwrap();
    // Keep the original complete local block, changing only its checksum.
    assert!(damaged_update.len() > 5);
    damaged_update[4] ^= 1;
    fs::write(&updates, &damaged_update).unwrap();
    let result = exchange.accept(&phone, reply);
    eprintln!(
        "refresh={result:?}; snapshot_same={}; updates_same={}; local_comments={}",
        fs::read(&snapshot).unwrap() == saved_snapshot,
        fs::read(&updates).ok().as_ref() == Some(&damaged_update),
        comments(&phone, &uuid)
    );
    assert!(
        result.is_err(),
        "a damaged local update must refuse the fresh replacement"
    );
    assert_eq!(fs::read(snapshot).unwrap(), saved_snapshot);
    assert_eq!(fs::read(updates).unwrap(), damaged_update);
}

#[test]
fn a_fresh_fetch_keeps_a_truncated_unsent_block_unchanged() {
    let (root, _b, phone, computer) = pair();
    let uuid = pre_root_pair(&phone, &computer);
    let mut exchange = Exchange::new();
    let fresh = refuse_then_request_fresh(&mut exchange, &phone, &computer, &uuid);
    let reply = answer(&computer, fresh).unwrap();
    comment(&phone, &uuid, "Unsent comment in a cut block");
    let snapshot = task_file(root.path(), &uuid);
    let updates = snapshot.with_extension("updates");
    let saved = fs::read(&snapshot).unwrap();
    let mut cut = fs::read(&updates).unwrap();
    cut.pop().unwrap();
    fs::write(&updates, &cut).unwrap();
    let error = exchange.accept(&phone, reply).unwrap_err().to_string();
    assert!(error.contains("updates are not whole"), "{error}");
    assert_eq!(fs::read(snapshot).unwrap(), saved);
    assert_eq!(fs::read(updates).unwrap(), cut);
}

#[test]
fn a_fresh_fetch_keeps_a_complete_but_unapplied_unsent_block_unchanged() {
    let (root, _b, phone, computer) = pair();
    let uuid = pre_root_pair(&phone, &computer);
    let mut exchange = Exchange::new();
    let fresh = refuse_then_request_fresh(&mut exchange, &phone, &computer, &uuid);
    let reply = answer(&computer, fresh).unwrap();
    comment(&phone, &uuid, "The missing dependency");
    comment(&phone, &uuid, "An unsent comment depending on it");
    let snapshot = task_file(root.path(), &uuid);
    let updates = snapshot.with_extension("updates");
    let saved = fs::read(&snapshot).unwrap();
    let blocks = fs::read(&updates).unwrap();
    // Remove the first whole block, leaving the second one's framing and
    // checksum intact but its dependency absent from the saved snapshot.
    let first = 8 + u32::from_le_bytes(blocks[..4].try_into().unwrap()) as usize;
    assert!(first < blocks.len());
    let pending = blocks[first..].to_vec();
    fs::write(&updates, &pending).unwrap();
    let error = exchange.accept(&phone, reply).unwrap_err().to_string();
    assert!(error.contains("cannot be read"), "{error}");
    assert_eq!(fs::read(snapshot).unwrap(), saved);
    assert_eq!(fs::read(updates).unwrap(), pending);
}
